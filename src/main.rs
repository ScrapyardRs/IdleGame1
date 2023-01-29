#![feature(async_fn_in_trait)]

use crate::game::GameFactory;
use crate::logger::LoggerOptions;
use drax::prelude::ErrorType;
use log::LevelFilter;
use mcprotocol::clientbound::play::ClientboundPlayRegistry::PlayerAbilities;
use mcprotocol::common::play::{GameType, Location, SimpleLocation};
use mcprotocol::{combine, msg};
use shovel::client::ProcessedPlayer;
use shovel::entity::tracking::TrackableEntity;
use shovel::phase::play::{ClientLoginProperties, ConnectedPlayer};
use shovel::spawn_server;
use shovel::status_builder;
use shovel::system::System;
use tokio::join;
use tokio::sync::mpsc::UnboundedSender;

// mod _scrapped_lobby;
mod game;
mod logger;

fn main() {
    logger::attach_system_logger(LoggerOptions {
        log_level: LevelFilter::Info,
        log_file: None,
    })
    .unwrap();

    log::info!("System logger attached.");

    // game factory
    log::info!("Bootstrapping game factory.");
    let (factory_sender, _) = GameFactory::bootstrap(());

    log::info!("Creating network runtime.");
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .enable_io()
        .build()
        .unwrap()
        .block_on(async move {
            if let Err(err) = spawn_server! {
                factory_sender,
                @bind "0.0.0.0:25565",
                @mc_status |count| status_builder! {
                    description: combine!(
                        msg!("Idle Game!\n", "#ffbbbb").bold(true),
                        msg!("Used For showing off ScrapyardRs", "#bbbbff").italic(true)
                    ).into(),
                    max: count + 1,
                    online: count,
                },
                @initial_location Location {
                    inner_loc: SimpleLocation {
                        x: 8.0,
                        y: 1.0,
                        z: 8.0,
                    },
                    yaw: 0.0,
                    pitch: 0.0,
                },
                @chunk_radius 8,
                factory_tx, client -> {
                    acquire_client(factory_tx, client).await?;
                    Ok(())
                }
            } {
                if !matches!(err.error_type, ErrorType::EOF) {
                    log::error!("Error running server: {}", err);
                }
            }
        })
}

async fn acquire_client(
    game_tx: UnboundedSender<ConnectedPlayer>,
    mut client: ProcessedPlayer,
) -> drax::prelude::Result<()> {
    client
        .send_client_login(
            "Example brand",
            ClientLoginProperties {
                hardcore: false,
                game_type: GameType::Creative,
                seed: 0,
                max_players: 20,
                simulation_distance: 20,
                reduced_debug_info: false,
                show_death_screen: false,
                is_debug: false,
                is_flat: false,
                last_death_location: None,
            },
        )
        .await?;
    client
        .server_player
        .writer
        .write_play_packet(&PlayerAbilities {
            flags: 0x1 | 0x2 | 0x4,
            flying_speed: 0.05,
            walking_speed: 0.1,
        })
        .await?;

    let (mut client, read_handle, write_handle) = client.keep_alive().await;

    // send initial position packet so the client can start loading in chunks
    client.teleport(client.location(), true).await;

    if game_tx.send(client).is_err() {
        return Ok(());
    }

    let _ = join!(read_handle, write_handle);
    Ok(())
}
