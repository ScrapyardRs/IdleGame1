mod session;

use crate::game::session::GameSession;
use mcprotocol::common::chunk::{CachedLevel, Chunk};
use mcprotocol::common::play::{Location, SimpleLocation};
use mcprotocol::common::registry::RegistryKey;
use shovel::phase::play::ConnectedPlayer;
use shovel::system::{System, TickResult};
use std::sync::Arc;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

#[derive(Clone)]
pub struct GameLevel {
    pub level: Arc<CachedLevel>,
    pub spawn: Location,
}

pub struct GameFactory {
    initial_client_recv: UnboundedReceiver<ConnectedPlayer>,
    level: GameLevel,
}

impl System for GameFactory {
    type CreationDetails = ();
    type SplitOff = UnboundedSender<ConnectedPlayer>;

    fn create(_: Self::CreationDetails) -> (Self, Self::SplitOff) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        let mut level = CachedLevel::default();
        let mut chunk = Chunk::new(0, 0);

        let global_block = RegistryKey::BlockStates
            .global("minecraft:chiseled_deepslate")
            .unwrap();
        chunk.rewrite_plane(0, global_block).unwrap();

        for x in -2..=2 {
            let mut chunk = chunk.clone_for(x, 0);
            if x == -2 || x == 2 {
                for y in 1..5 {
                    for o in 0..16 {
                        if x == -2 {
                            chunk.set_block_id(0, y, o, global_block).unwrap();
                        }
                        if x == 2 {
                            chunk.set_block_id(15, y, o, global_block).unwrap();
                        }
                    }
                }
            }

            for z in -2..=2 {
                let mut chunk = chunk.clone_for(x, z);
                // if x is -3 we need to draw on the -z axis for 0
                if z == -2 || z == 2 {
                    for y in 1..5 {
                        for o in 0..16 {
                            if z == -2 {
                                chunk.set_block_id(o, y, 0, global_block).unwrap();
                            }
                            if z == 2 {
                                chunk.set_block_id(o, y, 15, global_block).unwrap();
                            }
                        }
                    }
                }
                level.insert_chunk(chunk);
            }
        }

        let spawn = Location {
            inner_loc: SimpleLocation {
                x: 8.0,
                y: 1.0,
                z: 8.0,
            },
            yaw: 0.0,
            pitch: 0.0,
        };

        (
            Self {
                initial_client_recv: rx,
                level: GameLevel {
                    level: Arc::new(level),
                    spawn,
                },
            },
            tx,
        )
    }

    async fn tick(&mut self) -> TickResult {
        let next_player = self.initial_client_recv.recv().await;
        if next_player.is_none() {
            return TickResult::Stop;
        }
        let player = next_player.unwrap();
        GameSession::new(player, self.level.clone());
        TickResult::Continue
    }
}
