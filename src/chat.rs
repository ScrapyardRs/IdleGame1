use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::task::{Context, Poll};

use drax::prelude::Uuid;
use mcprotocol::clientbound::play::{ClientboundPlayRegistry, PlayerInfoEntry, PlayerInfoUpsert};
use mcprotocol::common::bit_set::BitSet;
use mcprotocol::common::chat::Chat;
use mcprotocol::common::GameProfile;
use mcprotocol::serverbound::play::ServerboundPlayRegistry;
use mcprotocol::{combine, msg};
use shovel::tick::{AwaitingEntity, CaptureAwaitingEntity, EntityFactory};
use shovel::PacketSend;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use crate::ranks::Rank;

pub enum ChatHandlerPacket {
    BroadcastMessage(Chat),
    BroadcastConditionalMessage(Chat, fn(&ChatHandlerEntity) -> bool),
    NewClient(ChatHandlerEntityStub),
    UpdateRank(Uuid, Rank),
}

pub struct ChatHandlerEntityStub {
    pub(crate) packet_recv: UnboundedReceiver<ServerboundPlayRegistry>,
    pub(crate) packet_send: UnboundedSender<ServerboundPlayRegistry>,
    pub(crate) write_clone: PacketSend,
    pub(crate) profile: GameProfile,
    pub(crate) init_ack: tokio::sync::oneshot::Sender<()>,
}

pub struct ChatHandlerEntity {
    packet_recv: UnboundedReceiver<ServerboundPlayRegistry>,
    packet_send: UnboundedSender<ServerboundPlayRegistry>,
    rank: Rank,
    write_clone: PacketSend,
    profile: GameProfile,
    init_ack: Option<tokio::sync::oneshot::Sender<()>>,
    pending_messages: VecDeque<String>,
    active: bool,
}

impl ChatHandlerEntity {
    fn entry(&self) -> PlayerInfoEntry {
        PlayerInfoEntry {
            profile_id: self.profile.id,
            profile: Some(self.profile.clone()),
            latency: Some(0),
            listed: Some(true),
            game_mode: Some(0),
            display_name: Some(self.display_name()),
            chat_session: None,
        }
    }

    fn display_name(&self) -> Chat {
        self.rank.format_name(self.profile.name.clone())
    }

    fn style_chat_content(&self, content: String) -> Chat {
        let display_name = self.display_name();
        combine!(display_name, msg!(" ").into(), msg!(content).into()).into()
    }
}

impl AwaitingEntity for ChatHandlerEntity {
    fn poll_tick(&mut self, cx: &mut Context) -> Result<bool, ()> {
        if !self.active {
            return Ok(false);
        }

        let mut ready = false;
        while match self.packet_recv.poll_recv(cx) {
            Poll::Ready(packet) => match packet {
                None => {
                    self.active = false;
                    return Err(());
                }
                Some(packet) => match packet {
                    ServerboundPlayRegistry::Chat { message, .. } => {
                        if message.eq("stop")
                            && self.profile.name.eq_ignore_ascii_case("DockerContainer")
                        {
                            std::process::exit(0)
                        }
                        self.pending_messages.push_back(message);
                        ready = true;
                        true
                    }
                    ServerboundPlayRegistry::ChatSessionUpdate { .. } => true,
                    packet => {
                        if let Err(_) = self.packet_send.send(packet) {
                            self.active = false;
                            return Err(());
                        }
                        true
                    }
                },
            },
            Poll::Pending => false,
        } {}
        Ok(ready)
    }
}

impl CaptureAwaitingEntity for ChatHandlerEntity {
    type AwaitingEntityOutput<'a> = &'a mut ChatHandlerEntity;

    #[allow(clippy::needless_lifetimes)]
    fn capture<'a>(&'a mut self) -> Self::AwaitingEntityOutput<'a> {
        self
    }
}

pub struct TamedChatHandler<'a> {
    packet_recv: &'a mut UnboundedReceiver<ChatHandlerPacket>,
    new_client_queue: &'a mut VecDeque<ChatHandlerEntityStub>,
    new_messages: &'a mut VecDeque<(Chat, fn(&ChatHandlerEntity) -> bool)>,
    update_rank_reqs: &'a mut Vec<(Uuid, Rank)>,
}

impl<'a> AwaitingEntity for TamedChatHandler<'a> {
    fn poll_tick(&mut self, cx: &mut Context) -> Result<bool, ()> {
        let mut needs_state_tick = false;
        while let Some(packet) = match self.packet_recv.poll_recv(cx) {
            Poll::Ready(Some(packet)) => Some(packet),
            Poll::Ready(None) => return Err(()),
            Poll::Pending => None,
        } {
            match packet {
                ChatHandlerPacket::BroadcastMessage(message) => {
                    needs_state_tick = true;
                    self.new_messages.push_back((message, |_| true));
                }
                ChatHandlerPacket::BroadcastConditionalMessage(message, condition) => {
                    needs_state_tick = true;
                    self.new_messages.push_back((message, condition));
                }
                ChatHandlerPacket::NewClient(client) => {
                    needs_state_tick = true;
                    self.new_client_queue.push_back(client);
                }
                ChatHandlerPacket::UpdateRank(id, rank) => {
                    needs_state_tick = true;
                    self.update_rank_reqs.push((id, rank));
                }
            }
        }
        Ok(needs_state_tick)
    }
}

pub struct ChatHandler {
    packet_recv: UnboundedReceiver<ChatHandlerPacket>,
    entities: HashMap<Uuid, ChatHandlerEntity>,
    new_client_queue: VecDeque<ChatHandlerEntityStub>,
    new_messages: VecDeque<(Chat, fn(&ChatHandlerEntity) -> bool)>,
    update_rank_reqs: Vec<(Uuid, Rank)>,
}

struct InnerBroadcastPacket {
    packet: Arc<ClientboundPlayRegistry>,
    predicate: fn(&ChatHandlerEntity) -> bool,
}

fn default_bit_set() -> BitSet {
    let mut bit_set = BitSet::value_of(vec![]).unwrap();
    bit_set.set(0).unwrap();
    bit_set.set(2).unwrap();
    bit_set.set(3).unwrap();
    bit_set.set(4).unwrap();
    bit_set.set(5).unwrap();
    bit_set
}

impl ChatHandler {
    async fn execute_handler_loop(&mut self) {
        loop {
            if !self.tick().await {
                break;
            }
            let current_clients_packet = if !self.new_client_queue.is_empty() {
                let mut entries = vec![];
                for client in self.entities.values() {
                    entries.push(client.entry());
                }
                Some(Arc::new(ClientboundPlayRegistry::PlayerInfoUpdate {
                    upsert: PlayerInfoUpsert {
                        actions: default_bit_set(),
                        entries,
                    },
                }))
            } else {
                None
            };
            let mut new_entries = vec![];
            let mut broadcast_packets = vec![];
            while let Some(client) = self.new_client_queue.pop_front() {
                if let Err(_) = client
                    .write_clone
                    .send(current_clients_packet.as_ref().unwrap().clone())
                {
                    continue;
                }
                let entity = ChatHandlerEntity {
                    packet_recv: client.packet_recv,
                    packet_send: client.packet_send,
                    rank: Rank::Default,
                    write_clone: client.write_clone,
                    profile: client.profile,
                    init_ack: Some(client.init_ack),
                    pending_messages: Default::default(),
                    active: true,
                };
                new_entries.push(entity.entry());
                self.entities.insert(entity.profile.id.clone(), entity);
            }

            let mut updated_ranks = vec![];
            for (id, rank) in &self.update_rank_reqs {
                if let Some(entity) = self.entities.get_mut(id) {
                    entity.rank = *rank;
                    updated_ranks.push(entity.entry());
                }
            }

            if !updated_ranks.is_empty() {
                let mut gamemode_bits = BitSet::value_of(vec![]).unwrap();
                gamemode_bits.set(5).unwrap();

                broadcast_packets.push(InnerBroadcastPacket {
                    packet: Arc::new(ClientboundPlayRegistry::PlayerInfoUpdate {
                        upsert: PlayerInfoUpsert {
                            actions: gamemode_bits,
                            entries: updated_ranks,
                        },
                    }),
                    predicate: |_| true,
                });
            }

            broadcast_packets.push(InnerBroadcastPacket {
                packet: Arc::new(ClientboundPlayRegistry::PlayerInfoUpdate {
                    upsert: PlayerInfoUpsert {
                        actions: default_bit_set(),
                        entries: new_entries,
                    },
                }),
                predicate: |_| true,
            });

            let mut clients_to_remove = vec![];
            for (id, client) in &mut self.entities {
                if !client.active {
                    clients_to_remove.push(id.clone());
                    continue;
                }

                while let Some(pending_message) = client.pending_messages.pop_front() {
                    self.new_messages
                        .push_back((client.style_chat_content(pending_message.clone()), |_| true));
                }
            }
            for id in &clients_to_remove {
                self.entities.remove(id);
            }
            let mass_remove = Arc::new(ClientboundPlayRegistry::PlayerInfoRemove {
                profile_ids: clients_to_remove,
            });
            while let Some((message, predicate)) = self.new_messages.pop_front() {
                broadcast_packets.push(InnerBroadcastPacket {
                    packet: Arc::new(ClientboundPlayRegistry::SystemChat {
                        content: message,
                        overlay: false,
                    }),
                    predicate,
                });
            }
            for (_, client) in &mut self.entities {
                macro_rules! match_packet {
                    ($packet:expr) => {
                        if let Err(_) = client.write_clone.send($packet.clone()) {
                            client.active = false;
                            continue;
                        };
                    };
                }

                match_packet!(mass_remove);

                for packet in broadcast_packets.iter() {
                    if (packet.predicate)(client) {
                        match_packet!(packet.packet);
                    }
                }

                if let Some(x) = client.init_ack.take() {
                    let _ = x.send(());
                }
            }
            self.new_messages.clear();
        }
    }
}

impl EntityFactory for ChatHandler {
    type Base<'a> = TamedChatHandler<'a>;
    type Entity = ChatHandlerEntity;

    #[allow(clippy::needless_lifetimes)]
    fn split_factory_mut<'a>(&'a mut self) -> (Self::Base<'a>, Vec<&'a mut Self::Entity>) {
        let tamed = TamedChatHandler {
            packet_recv: &mut self.packet_recv,
            new_client_queue: &mut self.new_client_queue,
            new_messages: &mut self.new_messages,
            update_rank_reqs: &mut self.update_rank_reqs,
        };
        let entities = self.entities.values_mut().collect::<Vec<_>>();
        (tamed, entities)
    }
}

pub fn create_global_chat_handle() -> UnboundedSender<ChatHandlerPacket> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let mut chat_handler = ChatHandler {
        packet_recv: rx,
        entities: Default::default(),
        new_client_queue: Default::default(),
        new_messages: Default::default(),
        update_rank_reqs: Default::default(),
    };
    tokio::spawn(async move { chat_handler.execute_handler_loop().await });
    tx
}
