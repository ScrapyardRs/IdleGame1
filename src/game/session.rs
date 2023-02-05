use std::ops::{Deref, DerefMut};
use std::time::Duration;

use mcprotocol::clientbound::play::ClientboundPlayRegistry::{InitializeBorder, TabList};
use mcprotocol::common::play::BlockPos;
use mcprotocol::msg;
use shovel::entity::tracking::{EntityData, EntityTracker, TrackableEntity};
use shovel::inventory::item::ItemBuilder;
use shovel::phase::play::ConnectedPlayer;
use tokio::sync::mpsc::error::TryRecvError;
use tokio::time::{interval, MissedTickBehavior};

use crate::chat::ChatHandlerPacket;
use crate::console::ConsolePacket;
use crate::db::{DbHook, PlayerDbInformation};
use crate::game::blocks::{BlockSystem, PlayerBlockData, GLOBAL_BLOCK_REGISTRY};
use crate::game::grip_item::{GripItem, GRIP_ITEM_REGISTRY};
use crate::game::stateful::{GlobPlayerState, StatefulEvent};
use crate::game::{ClientRouting, GameLevel};
use crate::ranks::Rank;

pub struct GameSessionPlayer {
    inner: ConnectedPlayer,
    pub current_tick: usize,
    db_hook: DbHook<PlayerDbInformation>,
    routing: ClientRouting,
    // extra data
    top_level_change: bool,
    pub rank: Rank,
    pub block_data: PlayerBlockData,
    pub grip_item: GripItem,
    // player state
    pub state: GlobPlayerState,
}

impl Into<PlayerDbInformation> for &mut GameSessionPlayer {
    fn into(self) -> PlayerDbInformation {
        PlayerDbInformation {
            uuid: self.uuid(),
            name: self.username().to_string(),
            rank: self.rank,
            block_data: self.block_data.clone(),
            grip_item: self.grip_item.clone(),
        }
    }
}

async fn setup_client_aesthetics(player: &mut ConnectedPlayer) {
    player.write_owned_packet(TabList {
        header: msg!("Welcome to my Block Game\n").into(),
        footer: msg!("\nPowered by scrapyard.rs").into(),
    })
}

impl GameSessionPlayer {
    pub fn target(&self) -> &str {
        self.username().as_str()
    }

    fn changed(&self) -> bool {
        self.block_data.changed || self.top_level_change
    }

    fn unchanged(&mut self) {
        self.block_data.changed = false;
        self.top_level_change = false;
    }

    pub fn save(&mut self) {
        let reserve: PlayerDbInformation = (self).into();
        let _ = self.db_hook.insert(&reserve);
    }

    pub async fn tick(
        &mut self,
        world: &GameLevel,
        tracker: &mut EntityTracker,
        block_system: &mut BlockSystem,
    ) {
        while let Some(console_packet) = match self.routing.console.try_recv() {
            Ok(next) => Some(next),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        } {
            match console_packet {
                ConsolePacket::UpdateRank(rank) => {
                    self.rank = rank;
                    let _ = self
                        .routing
                        .chat
                        .send(ChatHandlerPacket::UpdateRank(self.uuid(), rank));
                    self.top_level_change = true;
                }
            }
        }

        if self.current_tick == 0 {
            self.render_proxy_level(&world.level, world.spawn).await;
            if !self.is_loaded() {
                return;
            }
            self.current_tick += 1;

            let current_grip_item = self.grip_item.create_item().build();
            self.set_player_inventory_slot(Some(current_grip_item), 0, 3);
            self.set_player_inventory_slot(
                Some(
                    ItemBuilder::new("minecraft:comparator")
                        .display_name(msg!("Main Menu", "aqua").bold(true))
                        .build(),
                ),
                8,
                3,
            );

            self.teleport(world.spawn, true).await;
            tracker.add_player(&self.inner, self.inner.packets.clone_writer());

            let border_size = 5.0 * 16.0;
            self.write_owned_packet(InitializeBorder {
                new_center_x: 8.0,
                new_center_z: 8.0,
                old_size: border_size,
                new_size: border_size,
                lerp_time: 0,
                new_absolute_max_size: border_size as i32 + 1,
                warning_blocks: 1,
                warning_time: 0,
            });

            setup_client_aesthetics(self).await;
        } else {
            self.current_tick += 1;
            self.render_level(&world.level).await;
        }

        if self.current_tick % 100 == 0 && self.changed() {
            self.save();
            self.unchanged();
        }

        block_system.tick_for(BlockPos { x: 8, y: 0, z: 24 }, self);

        for stateful_event in self.state.tick(
            &mut self.inner,
            self.current_tick,
            block_system,
            &world.level,
            &self.block_data,
        ) {
            match stateful_event {
                StatefulEvent::BlockBroken(_, block) => {
                    let mined = &mut self.block_data.mined_blocks;
                    if mined.len() <= block.block_ordinal {
                        mined.resize(block.block_ordinal + 1, 0);
                    }
                    mined[block.block_ordinal] += 1;
                    self.top_level_change = true;
                }
            }
        }

        let (chunk_x, chunk_z) = (
            self.location().inner_loc.x as i32 >> 4,
            self.location().inner_loc.z as i32 >> 4,
        );
        if self.location().inner_loc.y < 0.0
            || chunk_x > 3
            || chunk_z > 3
            || chunk_x < -3
            || chunk_z < -3
        {
            self.teleport_local(world.spawn).await;
        }
    }
}

impl Deref for GameSessionPlayer {
    type Target = ConnectedPlayer;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for GameSessionPlayer {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

pub struct GameSession {
    host: GameSessionPlayer,
    world: GameLevel,
    tracker: EntityTracker,
    block_system: BlockSystem,
}

impl GameSession {
    pub fn new(routing: ClientRouting, player: ConnectedPlayer, world: GameLevel) {
        let db_hook = DbHook::player(player.uuid());
        let current = if db_hook.hook_path.exists() {
            let current = db_hook.load().unwrap().unwrap();
            if current.rank != Rank::Default {
                let _ = routing
                    .chat
                    .send(ChatHandlerPacket::UpdateRank(player.uuid(), current.rank));
            }
            current
        } else {
            let mut info = PlayerDbInformation {
                uuid: player.uuid(),
                name: player.username().to_string(),
                rank: Rank::Default,
                block_data: Default::default(),
                grip_item: GRIP_ITEM_REGISTRY.get(0).unwrap().clone(),
            };
            for reg_item in GLOBAL_BLOCK_REGISTRY.get_all() {
                if reg_item.is_default {
                    info.block_data.unlocked_blocks.push(reg_item.block_data);
                }
            }
            info
        };

        let mut game_session = GameSession {
            host: GameSessionPlayer {
                db_hook,
                inner: player,
                current_tick: 0,
                block_data: current.block_data,
                routing,
                top_level_change: false,
                rank: current.rank,
                grip_item: current.grip_item,
                state: GlobPlayerState::default(),
            },
            world,
            tracker: Default::default(),
            block_system: BlockSystem::default(),
        };

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_millis(50));
            interval.set_missed_tick_behavior(MissedTickBehavior::Burst);
            loop {
                if !game_session.tick().await {
                    game_session.host.save();
                    break;
                }
                interval.tick().await;
            }
        });
    }

    #[must_use]
    pub async fn tick(&mut self) -> bool {
        self.host
            .tick(&self.world, &mut self.tracker, &mut self.block_system)
            .await;

        self.tracker.tick(|uuid| {
            if uuid.eq(&self.host.uuid()) {
                Some(EntityData {
                    entity_location: self.host.location(),
                    entity_on_ground: self.host.on_ground(),
                })
            } else {
                None
            }
        });
        self.host.packets.active
    }
}
