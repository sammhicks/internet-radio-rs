use std::future::Future;

use anyhow::{Context, Result};
use futures::{future::Either, SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc, watch};
use warp::Filter;

use rradio_messages::{Command, LogMessage};

use crate::atomic_string::AtomicString;
use crate::log_error::log_error;
use crate::pipeline::{LogMessageSource, PlayerState};

use super::IncomingMessage;

trait ToDebugString {
    fn to_debug_string(&self) -> String;
}

impl<T: std::fmt::Debug> ToDebugString for T {
    fn to_debug_string(&self) -> String {
        format!("{:?}", self)
    }
}

enum OutgoingMessage {
    OutgoingMessage(
        rradio_messages::OutgoingMessage<
            &'static str,
            crate::atomic_string::AtomicString,
            super::TrackList,
        >,
    ),
    Pong(Vec<u8>),
}

#[allow(clippy::too_many_lines)]
pub async fn run(
    commands: mpsc::UnboundedSender<Command>,
    player_state: watch::Receiver<PlayerState>,
    log_message: LogMessageSource,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let commands = warp::any().map(move || commands.clone());
    let player_state = warp::any().map(move || player_state.clone());
    let log_message = warp::any().map(move || log_message.subscribe());

    let events = warp::ws()
        .and(commands)
        .and(player_state)
        .and(log_message)
        .map(
            |ws: warp::ws::Ws,
             commands: mpsc::UnboundedSender<Command>,
             player_state: watch::Receiver<PlayerState>,
             log_message: broadcast::Receiver<LogMessage<AtomicString>>| {
                ws.on_upgrade(|websocket| {
                    log_error(async move {
                        let (ws_tx, mut ws_rx) = websocket.split();

                        let ws_tx = ws_tx
                            .sink_map_err(|err| {
                                anyhow::Error::new(err).context("Could not send Websocket Message")
                            })
                            .with(|message: OutgoingMessage| async move {
                                match message {
                                    OutgoingMessage::OutgoingMessage(message) => {
                                        rmp_serde::to_vec(&message).map(warp::ws::Message::binary)
                                    }
                                    OutgoingMessage::Pong(pong) => {
                                        Ok(warp::ws::Message::binary(pong))
                                    }
                                }
                                .map_err(anyhow::Error::new)
                            });

                        pin_utils::pin_mut!(ws_tx);

                        ws_tx
                            .send(OutgoingMessage::OutgoingMessage(
                                rradio_messages::protocol_version_message(),
                            ))
                            .await?;

                        let mut current_state = (*player_state.borrow()).clone();

                        ws_tx
                            .send(OutgoingMessage::OutgoingMessage(
                                super::state_to_diff(&current_state).into(),
                            ))
                            .await?;

                        let (mut tx, mut rx) = mpsc::channel(1);

                        tokio::spawn(log_error(async move {
                            while let Some(message) = ws_rx.next().await {
                                let message = message?;

                                if message.is_close() {
                                    log::info!("Close message received");
                                    break;
                                }

                                if message.is_ping() {
                                    tx.send(message.into_bytes()).await.ok();
                                    continue;
                                }

                                let command: Command =
                                    match rmp_serde::from_slice(message.as_bytes())
                                        .context("Command not encoded using MsgPack")
                                    {
                                        Ok(c) => c,
                                        Err(err) => {
                                            log::error!("{:#}", err);
                                            continue;
                                        }
                                    };

                                if commands.send(command).is_err() {
                                    break;
                                }
                            }

                            Ok(())
                        }));

                        let player_state = player_state.map(IncomingMessage::StateUpdate);
                        let log_message = log_message.into_stream().filter_map(|msg| async {
                            match msg {
                                Ok(msg) => Some(IncomingMessage::LogMessage(msg)),
                                Err(_) => None,
                            }
                        });

                        pin_utils::pin_mut!(player_state);
                        pin_utils::pin_mut!(log_message);

                        let mut external_messages =
                            futures::stream::select(player_state, log_message);

                        loop {
                            let next_external_message = external_messages.next();
                            let next_internal_message = rx.next();

                            match futures::future::select(
                                next_external_message,
                                next_internal_message,
                            )
                            .await
                            {
                                Either::Left((
                                    Some(IncomingMessage::StateUpdate(new_state)),
                                    _,
                                )) => {
                                    let diff = super::diff_player_state(&current_state, &new_state);
                                    ws_tx
                                        .send(OutgoingMessage::OutgoingMessage(diff.into()))
                                        .await?;

                                    current_state = new_state;
                                }
                                Either::Left((
                                    Some(IncomingMessage::LogMessage(log_message)),
                                    _,
                                )) => {
                                    ws_tx
                                        .send(OutgoingMessage::OutgoingMessage(log_message.into()))
                                        .await?;
                                }
                                Either::Right((Some(ping), _)) => {
                                    log::info!("Ping: {:?}", ping);
                                    ws_tx.send(OutgoingMessage::Pong(ping)).await?;
                                }
                                Either::Left((None, _)) | Either::Right((None, _)) => break,
                            }
                        }

                        Ok(())
                    })
                })
            },
        );

    let filter = events;

    let addr = std::net::Ipv4Addr::LOCALHOST;
    let port = 8000;
    let socket_addr = (addr, port);

    warp::serve(filter)
        .bind_with_graceful_shutdown(socket_addr, shutdown)
        .1
        .await;

    Ok(())
}
