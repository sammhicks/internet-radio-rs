use anyhow::{Context, Result};
use futures::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use warp::Filter;

use rradio_messages::{Command, LogMessage};

use crate::atomic_string::AtomicString;
use crate::pipeline::{LogMessageSource, PlayerState};

use super::{Event, ShutdownSignal};

type OutgoingMessage = rradio_messages::OutgoingMessage<
    &'static str,
    crate::atomic_string::AtomicString,
    super::TrackList,
>;

trait ToDebugString {
    fn to_debug_string(&self) -> String;
}

impl<T: std::fmt::Debug> ToDebugString for T {
    fn to_debug_string(&self) -> String {
        format!("{:?}", self)
    }
}

#[allow(clippy::too_many_lines)]
pub async fn run(
    commands: mpsc::UnboundedSender<Command>,
    player_state: watch::Receiver<PlayerState>,
    log_message: LogMessageSource,
    shutdown_signal: ShutdownSignal,
) -> Result<()> {
    let commands = warp::any().map(move || commands.clone());
    let player_state = warp::any().map(move || player_state.clone());
    let log_message = warp::any().map(move || log_message.subscribe());
    let ws_shutdown_signal = {
        let shutdown_signal = shutdown_signal.clone();
        warp::any().map(move || shutdown_signal.clone())
    };

    let wait_group = super::wait_group::WaitGroup::new();

    let wait_handle = {
        let handle = wait_group.clone_handle();
        warp::any().map(move || handle.clone())
    };

    let events = warp::ws()
        .and(commands)
        .and(player_state)
        .and(log_message)
        .and(ws_shutdown_signal)
        .and(wait_handle)
        .map(
            |ws: warp::ws::Ws,
             commands: mpsc::UnboundedSender<Command>,
             player_state: watch::Receiver<PlayerState>,
             log_message: broadcast::Receiver<LogMessage<AtomicString>>,
             shutdown_signal: ShutdownSignal,
             wait_handle: super::wait_group::Handle| {
                ws.on_upgrade(|websocket| {
                    crate::log_error::log_error(async move {
                        let (ws_tx, mut ws_rx) = websocket.split();

                        let ws_tx = ws_tx
                            .sink_map_err(|err| {
                                anyhow::Error::new(err).context("Could not send Websocket Message")
                            })
                            .with(|message: OutgoingMessage| async move {
                                rmp_serde::to_vec(&message)
                                    .map(warp::ws::Message::binary)
                                    .map_err(anyhow::Error::new)
                            });

                        pin_utils::pin_mut!(ws_tx);

                        ws_tx
                            .send(rradio_messages::protocol_version_message())
                            .await?;

                        let mut current_state = (*player_state.borrow()).clone();

                        ws_tx
                            .send(super::player_state_to_diff(&current_state).into())
                            .await?;

                        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

                        wait_handle.spawn_task(async move {
                            while let Some(message) = ws_rx.next().await {
                                let message = message?;

                                if message.is_close() {
                                    log::debug!("Close message received");
                                    break;
                                }

                                commands
                                    .send(
                                        rmp_serde::from_slice(message.as_bytes())
                                            .context("Command not encoded using MsgPack")?,
                                    )
                                    .context("Failed to send command")?
                            }

                            drop(shutdown_tx);

                            Ok(())
                        });

                        let player_state = player_state.map(Event::StateUpdate);
                        let log_message = log_message.into_stream().filter_map(|msg| async {
                            match msg {
                                Ok(msg) => Some(Event::LogMessage(msg)),
                                Err(_) => None,
                            }
                        });

                        let events = futures::stream::select(player_state, log_message)
                            .take_until(shutdown_rx)
                            .take_until(shutdown_signal.wait());

                        pin_utils::pin_mut!(events);

                        while let Some(event) = events.next().await {
                            match event {
                                Event::StateUpdate(new_state) => {
                                    let diff = super::diff_player_state(&current_state, &new_state);
                                    ws_tx.send(diff.into()).await?;

                                    current_state = new_state;
                                }
                                Event::LogMessage(log_message) => {
                                    ws_tx.send(log_message.into()).await?;
                                }
                            }
                        }

                        ws_tx.close().await?;

                        drop(wait_handle);

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
        .bind_with_graceful_shutdown(socket_addr, shutdown_signal.wait())
        .1
        .await;

    log::debug!("Web server shut down");

    wait_group.wait().await;

    log::debug!("Websockets shut down");

    Ok(())
}
