use std::fmt::{Display, Formatter};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::db::ensure_db;
use bytes::{Buf, BytesMut};
use mcprotocol::common::GameProfile;
use pin_project_lite::pin_project;
use tokio::io::Stdin;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio_util::io::poll_read_buf;

use crate::ranks::Rank;

pub fn attach_console() -> UnboundedSender<ConsoleHandle> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    let console = Console {
        stdin: tokio::io::stdin(),
        current_buffer: BytesMut::new(),
        recv: rx,
        handles: Vec::new(),
    };
    tokio::spawn(async move {
        console.run().await;
    });
    tx
}

pub enum ConsolePacket {
    UpdateRank(Rank),
}

pub type ConsoleHandle = (GameProfile, UnboundedSender<ConsolePacket>);

pub struct Console {
    pub stdin: Stdin,
    pub current_buffer: BytesMut,
    pub recv: UnboundedReceiver<ConsoleHandle>,
    pub handles: Vec<ConsoleHandle>,
}

impl Console {
    pub fn poll(&mut self) -> ConsoleFuture {
        ConsoleFuture {
            stdin: &mut self.stdin,
            current_buffer: &mut self.current_buffer,
            recv: &mut self.recv,
            handles: &mut self.handles,
        }
    }

    pub async fn run(mut self) {
        loop {
            let commands = match self.poll().await {
                Ok(commands) => commands,
                Err(ConsoleFutureError::FailedUtfRead) => continue,
                Err(err) => {
                    log::error!("Error in console: {}", err);
                    continue;
                }
            };

            for command in commands {
                handle_command(command, &self.handles);
            }
        }
    }
}

fn handle_command(command: String, handles: &Vec<ConsoleHandle>) {
    log::info!("Handling command: {}", command);

    let mut split_up = command.split(" ");
    let command = match split_up.next() {
        Some(command) => command,
        None => return,
    };
    let args = split_up.collect::<Vec<_>>();
    match command {
        "rank" => handle_rank(args, handles),
        "help" => {
            log::info!("Available commands:");
            log::info!("help - show this message");
            log::info!("stop - stop the server");
            log::info!("rank <player> <rank> - set a player's rank");
        }
        "stop" => std::process::exit(1),
        _ => {
            log::info!("Unrecognized command.");
        }
    }
}

fn handle_rank(args: Vec<&str>, handles: &Vec<ConsoleHandle>) {
    if args.len() != 2 {
        println!("Usage: rank <player> <rank>");
        return;
    }
    let player = args[0];
    let rank = match args[1] {
        "default" => Rank::Default,
        "staff" => Rank::Staff,
        "owner" => Rank::Owner,
        _ => {
            log::info!("Invalid rank.");
            return;
        }
    };
    let mut found = false;
    for (profile, handle) in handles {
        if profile.name == player {
            found = true;
            let _ = handle.send(ConsolePacket::UpdateRank(rank));
            log::info!("Updated player's rank!");
            break;
        }
    }
    if !found {
        log::info!("Could not find player {}.", player);
    }
}

pin_project! {
    pub struct ConsoleFuture<'a> {
        stdin: &'a mut Stdin,
        current_buffer: &'a mut BytesMut,
        recv: &'a mut UnboundedReceiver<ConsoleHandle>,
        handles: &'a mut Vec<ConsoleHandle>,
    }
}

#[derive(Debug, Clone, Copy)]
pub enum ConsoleFutureError {
    RecvDropped,
    StdinFailedRead,
    FailedUtfRead,
}

impl Display for ConsoleFutureError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RecvDropped => write!(f, "Console recv dropped"),
            Self::StdinFailedRead => write!(f, "Stdin failed to read"),
            Self::FailedUtfRead => write!(f, "Failed to read utf8"),
        }
    }
}

impl std::error::Error for ConsoleFutureError {}

impl<'a> Future for ConsoleFuture<'a> {
    type Output = Result<Vec<String>, ConsoleFutureError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let me = self.project();
        me.handles.retain(|handle| !handle.1.is_closed());
        if let Some(handle) = match me.recv.poll_recv(cx) {
            Poll::Ready(Some(handle)) => Some(handle),
            Poll::Ready(None) => return Poll::Ready(Err(ConsoleFutureError::RecvDropped)),
            Poll::Pending => None,
        } {
            me.handles.push(handle);
        }

        loop {
            match poll_read_buf(Pin::new(me.stdin), cx, me.current_buffer) {
                Poll::Ready(Ok(_)) => {}
                Poll::Ready(Err(_)) => {
                    return Poll::Ready(Err(ConsoleFutureError::StdinFailedRead));
                }
                Poll::Pending => {
                    return Poll::Pending;
                }
            }

            let mut commands = vec![];
            loop {
                let mut iter = me.current_buffer.iter();
                let next = iter.position(|b| *b == b'\n');
                drop(iter);
                if let Some(pos) = next {
                    let pos = pos + 1;
                    let mut take = me.current_buffer.take(pos);
                    let str = match std::str::from_utf8(&take.chunk()[..pos - 1]) {
                        Ok(str) => str,
                        Err(_) => {
                            return Poll::Ready(Err(ConsoleFutureError::FailedUtfRead));
                        }
                    };
                    commands.push(str.to_string());
                    take.advance(pos);
                } else {
                    break;
                }
            }
            if !commands.is_empty() {
                return Poll::Ready(Ok(commands));
            }
        }
    }
}
