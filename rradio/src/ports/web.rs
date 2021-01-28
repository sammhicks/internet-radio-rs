use futures::{SinkExt, StreamExt};
use tokio::sync::oneshot;
use warp::Filter;

use super::Event;

use crate::task::FailableFuture;

trait ToDebugString {
    fn to_debug_string(&self) -> String;
}

impl<T: std::fmt::Debug> ToDebugString for T {
    fn to_debug_string(&self) -> String {
        format!("{:?}", self)
    }
}

pub async fn run(port_channels: super::PortChannels) {
    let wait_group = crate::task::WaitGroup::new();
    let wait_handle = wait_group.clone_handle();

    let ws_shutdown_signal = port_channels.shutdown_signal.clone();

    let events = warp::ws().map(move |ws: warp::ws::Ws| {
        let port_channels = port_channels.clone();
        let wait_handle = wait_handle.clone();
        ws.on_upgrade(|websocket| {
            async move {
                let (ws_tx, mut ws_rx) = websocket.split();

                let ws_tx = ws_tx
                    .sink_map_err(|err| {
                        anyhow::Error::new(err).context("Failed to send Websocket message")
                    })
                    .with(|event: super::BroadcastEvent| async move {
                        rmp_serde::to_vec_named(&event)
                            .map(warp::ws::Message::binary)
                            .map_err(anyhow::Error::new)
                    });

                tokio::pin!(ws_tx);

                ws_tx
                    .send(rradio_messages::protocol_version_message())
                    .await?;

                let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

                let commands = port_channels.commands;

                wait_handle.spawn_task(
                    async move {
                        while let Some(message) = ws_rx.next().await {
                            let message = message?;

                            if message.is_close() {
                                break;
                            }

                            commands.send(rmp_serde::from_slice(message.as_bytes())?)?
                        }

                        drop(shutdown_tx);

                        log::debug!("Close message received");

                        Ok(())
                    }
                    .log_error(std::module_path!()),
                );

                let events = super::event_stream(
                    port_channels.player_state.clone(),
                    port_channels.log_message_source.subscribe(),
                )
                .take_until(shutdown_rx)
                .take_until(port_channels.shutdown_signal.clone().wait());

                tokio::pin!(events);

                while let Some(event) = events.next().await {
                    match event {
                        Event::StateUpdate(diff) => {
                            ws_tx.send(diff.into()).await?;
                        }
                        Event::LogMessage(log_message) => {
                            ws_tx.send(log_message.into()).await?;
                        }
                    }
                }

                ws_tx.close().await?;

                drop(wait_handle);

                Ok(())
            }
            .log_error(std::module_path!())
        })
    });

    let filter = events;

    let addr = if cfg!(feature = "production-server") {
        std::net::Ipv4Addr::UNSPECIFIED
    } else {
        std::net::Ipv4Addr::LOCALHOST
    };

    let port = if cfg!(feature = "production-server") {
        80
    } else {
        8000
    };
    let socket_addr = (addr, port);

    let (server_addr, server) =
        warp::serve(filter).bind_with_graceful_shutdown(socket_addr, ws_shutdown_signal.wait());

    log::info!("Listening on {}", server_addr);

    server.await;

    log::debug!("Shutting down");

    wait_group.wait().await;

    log::debug!("Shut down");
}
