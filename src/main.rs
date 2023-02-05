#![feature(async_fn_in_trait)]
#![feature(once_cell)]

use drax::prelude::ErrorType;
use log::LevelFilter;
use mcprotocol::clientbound::play::ClientboundPlayRegistry::PlayerAbilities;
use mcprotocol::common::play::{GameType, Location, SimpleLocation};
use mcprotocol::{combine, msg};
use shovel::client::ProcessedPlayer;
use shovel::entity::tracking::TrackableEntity;
use shovel::phase::login::MinehutLoginServer;
use shovel::phase::play::{ClientLoginProperties, ConnectedPlayer};
use shovel::spawn_server;
use shovel::status_builder;
use shovel::system::System;
use tokio::join;
use tokio::sync::mpsc::UnboundedSender;

use crate::chat::{create_global_chat_handle, ChatHandlerEntityStub, ChatHandlerPacket};
use crate::console::{attach_console, ConsoleHandle};
use crate::game::{ClientRouting, GameFactory};
use crate::logger::LoggerOptions;

mod chat;
mod console;
mod db;
mod game;
mod logger;
mod ranks;
pub mod raytrace;

fn main() {
    db::ensure_db();

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
            let console = attach_console();
            let chat = create_global_chat_handle();

            if let Err(err) = spawn_server! {
                (console, factory_sender, chat), MinehutLoginServer,
                @proxy_protocol true,
                @bind "0.0.0.0:25575",
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
                ctx, client -> {
                    acquire_client(ctx, client).await?;
                    Ok(())
                }
            } {
                if !matches!(err.error_type, ErrorType::EOF) {
                    log::error!("Error running server: {}", err);
                }
            }
        });
}

async fn acquire_client(
    (console, factory_sender, chat): (
        UnboundedSender<ConsoleHandle>,
        UnboundedSender<(ClientRouting, ConnectedPlayer)>,
        UnboundedSender<ChatHandlerPacket>,
    ),
    mut client: ProcessedPlayer,
) -> drax::prelude::Result<()> {
    client
        .send_client_login(
            "Idle Game",
            ClientLoginProperties {
                hardcore: false,
                game_type: GameType::Survival,
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

    let write_clone = client.packets.clone_writer();
    let profile = client.profile().clone();
    let (ack_tx, ack_rx) = tokio::sync::oneshot::channel();

    let (console_tx, console_rx) = tokio::sync::mpsc::unbounded_channel();

    let _ = console.send((profile.clone(), console_tx));

    let chat_clone = chat.clone();
    client = client.mutate_receiver(move |recv| {
        let (ntx, nrx) = tokio::sync::mpsc::unbounded_channel();
        let _ = chat_clone.send(ChatHandlerPacket::NewClient(ChatHandlerEntityStub {
            packet_recv: recv,
            packet_send: ntx,
            write_clone,
            profile,
            init_ack: ack_tx,
        }));
        nrx
    });

    if ack_rx.await.is_err() {
        return Ok(());
    }

    // send initial position packet so the client can start loading in chunks
    client.teleport(client.location(), true).await;

    if factory_sender
        .send((
            ClientRouting {
                chat,
                console: console_rx,
            },
            client,
        ))
        .is_err()
    {
        return Ok(());
    }

    let _ = join!(read_handle, write_handle);
    Ok(())
}
