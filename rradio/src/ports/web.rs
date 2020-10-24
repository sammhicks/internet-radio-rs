use anyhow::Result;
use futures::{SinkExt, StreamExt};
use tokio::sync::oneshot;
use warp::Filter;

use super::{BroadcastEvent, Event};

trait ToDebugString {
    fn to_debug_string(&self) -> String;
}

impl<T: std::fmt::Debug> ToDebugString for T {
    fn to_debug_string(&self) -> String {
        format!("{:?}", self)
    }
}

pub async fn run(port_channels: super::PortChannels) -> Result<()> {
    let wait_group = super::wait_group::WaitGroup::new();
    let wait_handle = wait_group.clone_handle();

    let ws_shutdown_signal = port_channels.shutdown_signal.clone();

    let events = warp::ws().map(move |ws: warp::ws::Ws| {
        let port_channels = port_channels.clone();
        let wait_handle = wait_handle.clone();
        ws.on_upgrade(|websocket| {
            crate::log_error::log_error(async move {
                let (ws_tx, mut ws_rx) = websocket.split();

                let ws_tx = ws_tx
                    .sink_map_err(|err| {
                        anyhow::Error::new(err).context("Could not send Websocket Message")
                    })
                    .with(|event: BroadcastEvent| async move {
                        rmp_serde::to_vec(&event)
                            .map(warp::ws::Message::binary)
                            .map_err(anyhow::Error::new)
                    });

                pin_utils::pin_mut!(ws_tx);

                ws_tx
                    .send(rradio_messages::protocol_version_message())
                    .await?;

                let mut current_state = (*port_channels.player_state.borrow()).clone();

                ws_tx
                    .send(super::player_state_to_diff(&current_state).into())
                    .await?;

                let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

                let commands = port_channels.commands;

                wait_handle.spawn_task(async move {
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
                });

                let player_state = port_channels.player_state.map(Event::StateUpdate);
                let log_message = port_channels
                    .log_message_source
                    .subscribe()
                    .into_stream()
                    .filter_map(|msg| async {
                        match msg {
                            Ok(msg) => Some(Event::LogMessage(msg)),
                            Err(_) => None,
                        }
                    });

                let events = futures::stream::select(player_state, log_message)
                    .take_until(shutdown_rx)
                    .take_until(port_channels.shutdown_signal.wait());

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

    Ok(())
}
