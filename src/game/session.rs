use mcprotocol::clientbound::play::ClientboundPlayRegistry::{
    InitializeBorder, SetBorderCenter, SetBorderSize,
};
use std::ops::{Deref, DerefMut};
use std::time::Duration;

use shovel::entity::tracking::{EntityData, EntityTracker, TrackableEntity};
use shovel::phase::play::ConnectedPlayer;
use tokio::time::{interval, MissedTickBehavior};

use crate::game::GameLevel;

pub struct GameSessionPlayer {
    inner: ConnectedPlayer,
    current_tick: usize,
}

impl GameSessionPlayer {
    pub fn target(&self) -> &str {
        self.username().as_str()
    }

    pub async fn tick(&mut self, world: &GameLevel, tracker: &mut EntityTracker) {
        if self.current_tick == 0 {
            self.render_proxy_level(&world.level, world.spawn).await;
            if !self.is_loaded() {
                return;
            }
            self.current_tick += 1;
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
        } else {
            self.render_level(&world.level).await;
        }
        while let Some(packet) = self.next_packet() {
            if !self.is_loaded() {
                continue;
            }
            match packet {
                packet => {
                    log::info!(target: self.target(), "Unhandled lobby (client) packet: {:?}", packet);
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
}

impl GameSession {
    pub fn new(player: ConnectedPlayer, world: GameLevel) {
        let player_name = player.username().to_string();
        let mut game_session = GameSession {
            host: GameSessionPlayer {
                inner: player,
                current_tick: 0,
            },
            world,
            tracker: Default::default(),
        };

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_millis(50));
            interval.set_missed_tick_behavior(MissedTickBehavior::Burst);
            loop {
                if let Err(err) = game_session.tick().await {
                    log::error!(target: player_name.as_str(), "Error during game session tick {:?}", err);
                    return;
                }
                interval.tick().await;
            }
        });
    }

    pub async fn tick(&mut self) -> anyhow::Result<()> {
        self.host.tick(&self.world, &mut self.tracker).await;
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
        Ok(())
    }
}
