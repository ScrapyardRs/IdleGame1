use std::cmp::max;

use mcprotocol::clientbound::play::ClientboundPlayRegistry::{
    BlockChangedAck, BlockDestruction, BlockUpdate, SystemChat,
};
use mcprotocol::common::chunk::CachedLevel;
use mcprotocol::common::play::{BlockPos, InteractionHand};
use mcprotocol::serverbound::play::{PlayerActionType, PlayerCommandType, ServerboundPlayRegistry};
use mcprotocol::{combine, msg};
use shovel::entity::tracking::TrackableEntity;
use shovel::inventory::{ClickContext, ClickWith, Menu};
use shovel::phase::play::ConnectedPlayer;

use crate::game::blocks::{AvailableBlockData, BlockSystem, PlayerBlockData};
use crate::game::stateful::StatefulEvent::BlockBroken;

#[derive(Debug)]
pub enum StatefulEvent {
    BlockBroken(BlockPos, AvailableBlockData),
}

pub enum MenuState<C: Send + Sync> {
    None,
    Own,
    Other(Menu<C>),
}

impl<C: Send + Sync> Default for MenuState<C> {
    fn default() -> Self {
        MenuState::None
    }
}

#[derive(Default)]
pub struct GlobPlayerState {
    // states
    pub player_destroying_state: PlayerDestroyingState,
    // global state
    current_keepalive_seq: u64,
    current_menu: MenuState<()>,
}

// struct GlobalStateHandle<'a> {
//     _current_keep_alive_seq: u64,
//     _phantom_a: PhantomData<&'a ()>,
// }

impl GlobPlayerState {
    pub fn tick(
        &mut self,
        player: &mut ConnectedPlayer,
        _current_tick: usize,
        system: &mut BlockSystem,
        level: &CachedLevel,
        block_data: &PlayerBlockData,
    ) -> Vec<StatefulEvent> {
        // macro_rules! global_handle {
        //     () => {
        //         GlobalStateHandle {
        //             _current_keep_alive_seq: self.current_keepalive_seq,
        //             _phantom_a: Default::default(),
        //         }
        //     };
        // }

        let mut stateful_events = vec![];

        self.player_destroying_state.execute_ack(player);

        while let Some(packet) = player.next_packet() {
            if !player.is_loaded() {
                return vec![];
            }
            match packet {
                ServerboundPlayRegistry::ContainerClose { container_id } => {
                    match (container_id, &self.current_menu) {
                        (0, MenuState::Own) => {
                            self.current_menu = MenuState::None;
                        }
                        (x, MenuState::Other(menu)) if menu.container_id() == x => {
                            self.current_menu = MenuState::None;
                        }
                        _ => {}
                    }
                }
                ServerboundPlayRegistry::KeepAlive { keep_alive_id } => {
                    self.current_keepalive_seq = keep_alive_id;
                }
                ServerboundPlayRegistry::PlayerAbilities { .. } => {}
                ServerboundPlayRegistry::PlayerCommand { action_type, .. } => match action_type {
                    PlayerCommandType::OpenInventory => {
                        self.current_menu = MenuState::Own;
                    }
                    _ => {}
                },
                ServerboundPlayRegistry::UseItem { hand, .. }
                | ServerboundPlayRegistry::UseItemOn { hand, .. } => {
                    if matches!(hand, InteractionHand::MainHand) {
                        match player.player_inventory().current_slot {
                            0 => {
                                // todo upgrade stuff
                            }
                            8 => {
                                if let MenuState::Other(current) = &self.current_menu {
                                    if current.container_id() == 1 {
                                        continue;
                                    }
                                }
                                let menu = super::menus::mined_statistics_page(block_data);
                                menu.send_to_player(player);
                                self.current_menu = MenuState::Other(menu);
                            }
                            _ => {}
                        }
                    }
                }
                ServerboundPlayRegistry::Swing { hand } => {
                    if matches!(hand, InteractionHand::MainHand) {
                        match player.player_inventory().current_slot {
                            0 => {
                                if let Some(event) = self
                                    .player_destroying_state
                                    .continue_destroying(player, system)
                                {
                                    stateful_events.push(event);
                                }
                            }
                            8 => {
                                if let MenuState::Other(current) = &self.current_menu {
                                    if current.container_id() == 1 {
                                        continue;
                                    }
                                }
                                let menu = super::menus::mined_statistics_page(block_data);
                                menu.send_to_player(player);
                                self.current_menu = MenuState::Other(menu);
                            }
                            _ => {}
                        }
                    }
                }
                ServerboundPlayRegistry::PlayerAction {
                    action_type,
                    at_pos,
                    sequence,
                    ..
                } => match action_type {
                    PlayerActionType::StartDestroyBlock => {
                        self.player_destroying_state
                            .start_destroying(player, at_pos);
                        self.player_destroying_state.ack(sequence);
                    }
                    PlayerActionType::AbortDestroyBlock => {
                        self.player_destroying_state
                            .abort_destroying(player, system);
                        self.player_destroying_state.ack(sequence);
                    }
                    PlayerActionType::StopDestroyBlock => {
                        self.player_destroying_state.ack(sequence);
                        self.player_destroying_state
                            .stop_destroying(player, system, level);
                    }
                    PlayerActionType::DropAllItems
                    | PlayerActionType::DropItem
                    | PlayerActionType::SwapItemWithOffhand => {
                        player.refresh_player_inventory();
                    }
                    PlayerActionType::ReleaseUseItem => {}
                },
                ServerboundPlayRegistry::ContainerClick {
                    container_id,
                    state_id,
                    slot,
                    button,
                    action,
                    changed_slots,
                    carried_item,
                } => {
                    if container_id == 0 {
                        player.refresh_player_inventory();
                    } else {
                        if let MenuState::Other(menu) = &mut self.current_menu {
                            if let Some(clicker) = menu.get_clicker(state_id, slot) {
                                let click_context = ClickContext {
                                    extra: &mut (),
                                    player,
                                    menu_ref: menu,
                                    click_type: action,
                                    click_with: if button == 0 {
                                        ClickWith::Left
                                    } else {
                                        ClickWith::Right
                                    },
                                    slot,
                                    changed_slots,
                                    carried_item,
                                };
                                (clicker)(click_context);
                            }
                        }
                    }
                }
                ServerboundPlayRegistry::SetCarriedItem { mut slot } => {
                    if slot > 8 {
                        slot = 8
                    }
                    player
                        .player_inventory_mut()
                        .set_current_slot_unaware(slot as u8);
                }
                packet => {
                    log::info!(target: player.username().as_str(), "Unhandled state packet: {:?}", packet);
                }
            }
        }
        self.player_destroying_state.reset_tick();
        stateful_events
    }
}

#[derive(Debug)]
pub enum CurrentDestroyingState {
    None,
    Started(BlockPos),
    Aborted(BlockPos),
}

impl Default for CurrentDestroyingState {
    fn default() -> Self {
        CurrentDestroyingState::None
    }
}

#[derive(Default)]
pub struct PlayerDestroyingState {
    destroying_state: CurrentDestroyingState,
    pub destroying_block_sequence: Option<i32>,
    damage_this_tick: bool,
}

impl PlayerDestroyingState {
    fn reset_tick(&mut self) {
        self.damage_this_tick = false;
    }

    fn execute_ack(&mut self, player: &mut ConnectedPlayer) {
        if let Some(seq) = self.destroying_block_sequence.take() {
            player.write_owned_packet(BlockChangedAck { sequence_id: seq });
        }
    }

    fn ack(&mut self, seq: i32) {
        self.destroying_block_sequence = match self.destroying_block_sequence {
            None => Some(seq),
            Some(n) => Some(max(n, seq)),
        };
    }

    fn continue_destroying<'a>(
        &'a mut self,
        player: &'a mut ConnectedPlayer,
        system: &'a mut BlockSystem,
    ) -> Option<StatefulEvent> {
        if self.damage_this_tick {
            return None;
        }
        self.damage_this_tick = true;

        let mut lock_block_progress = false;

        let next_state = match &self.destroying_state {
            CurrentDestroyingState::None => {
                return None;
            }
            CurrentDestroyingState::Started(_) => None,
            CurrentDestroyingState::Aborted(target) => {
                let mut loc = player.location().clone();
                loc.inner_loc.y += 1.8 * 0.85;
                let ray_trace = crate::raytrace::RayTraceIterator::new(loc, 10.0);
                let mut found = None;
                for pos in ray_trace {
                    if pos == *target {
                        found = Some(CurrentDestroyingState::Started(*target));
                        break;
                    }
                }
                if found.is_some() {
                    system.reset_progress(player, *target);
                    let write_state = system
                        .current_state(player, *target)
                        .map(|x| x.block_id)
                        .unwrap_or(0);
                    player.write_owned_packet(BlockUpdate {
                        pos: *target,
                        state: write_state,
                    });
                    lock_block_progress = true;
                }
                match found {
                    Some(state) => Some(state),
                    None => Some(CurrentDestroyingState::None),
                }
            }
        };
        if let Some(next_state) = next_state {
            self.destroying_state = next_state;
        }

        if let CurrentDestroyingState::Started(target) = &self.destroying_state {
            if !lock_block_progress {
                let progress = system.attempt_damage_block(*target, player)?;
                // client does it's own predictions; sending them this is a waste
                // maybe we can implement it in the future
                //
                // let id = player.id();
                // let health = progress.0 as f64;
                // let initial_health = progress.1 as f64;
                // player.write_owned_packet(BlockDestruction {
                //     id,
                //     pos: *target,
                //     progress: (f64::floor((initial_health - health) / initial_health) * 9.0)
                //         as u8,
                // });

                if progress.0 == progress.1 {
                    let current = system.remove(player, *target)?;
                    player.write_owned_packet(BlockUpdate {
                        pos: *target,
                        state: 0,
                    }); // they've broken the block sufficiently to our standards
                    player.write_owned_packet(SystemChat {
                        content: combine!(msg!("Block Broken!", "aqua").bold(true)).into(),
                        overlay: true,
                    });
                    Some(BlockBroken(*target, current))
                } else {
                    player.write_owned_packet(SystemChat {
                        content: combine!(
                            msg!("Break Progress: ", "aqua").bold(true),
                            msg!(format!("{}", progress.0), "green"),
                            msg!("/", "aqua"),
                            msg!(format!("{}", progress.1), "green")
                        )
                        .into(),
                        overlay: true,
                    });
                    None
                }
            } else {
                None
            }
        } else {
            None
        }
    }

    fn start_destroying<'a>(&'a mut self, player: &'a mut ConnectedPlayer, at: BlockPos) {
        match self.destroying_state {
            CurrentDestroyingState::Started(pos) => {
                let id = player.id();
                player.write_owned_packet(BlockDestruction {
                    id,
                    pos,
                    progress: 255,
                });
            }
            _ => {}
        }
        self.destroying_state = CurrentDestroyingState::Started(at);
    }

    fn stop_destroying<'a>(
        &'a mut self,
        player: &'a mut ConnectedPlayer,
        system: &'a mut BlockSystem,
        level: &CachedLevel,
    ) {
        match self.destroying_state {
            CurrentDestroyingState::None => (),
            CurrentDestroyingState::Started(pos) | CurrentDestroyingState::Aborted(pos) => {
                let write_state = system
                    .current_state(player, pos)
                    .map(|x| x.block_id)
                    .unwrap_or(
                        level
                            .clone_necessary_chunk(pos.x >> 4, pos.z >> 4)
                            .map(|x| x.get_block_id(pos.x & 0xF, pos.y, pos.z & 0xF).unwrap_or(0))
                            .unwrap_or(0),
                    );
                player.write_owned_packet(BlockUpdate {
                    pos,
                    state: write_state,
                });
                self.execute_ack(player);
            }
        }
    }

    fn abort_destroying<'a>(
        &'a mut self,
        player: &'a mut ConnectedPlayer,
        system: &'a mut BlockSystem,
    ) {
        match self.destroying_state {
            CurrentDestroyingState::None => {
                return;
            }
            CurrentDestroyingState::Started(pos) | CurrentDestroyingState::Aborted(pos) => {
                self.destroying_state = CurrentDestroyingState::Aborted(pos);
                if system.reset_progress(player, pos).is_some() {
                    let id = player.id();
                    player.write_owned_packet(BlockDestruction {
                        id,
                        pos,
                        progress: 255,
                    });
                    let write_state = system
                        .current_state(player, pos)
                        .map(|x| x.block_id)
                        .unwrap_or(0);
                    player.write_owned_packet(BlockUpdate {
                        pos,
                        state: write_state,
                    });
                }
            }
        }
    }
}
