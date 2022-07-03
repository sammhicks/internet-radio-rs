use anyhow::Context;
use axum::{
    response::IntoResponse,
    routing::{get, get_service, post},
};
use futures::{FutureExt, SinkExt, StreamExt};
use tokio::sync::oneshot;

use crate::task::FailableFuture;

async fn handle_websocket_connection(
    port_channels: super::PortChannels,
    wait_handle: crate::task::WaitGroupHandle,
    websocket: axum::extract::ws::WebSocket,
) -> anyhow::Result<()> {
    let (websocket_tx, mut websocket_rx) = websocket.split();

    // Convert the websocket sink (i.e. of websocket [axum::extract::ws::Message]) into a sink of [`BroadcastEvent`]
    let websocket_tx = websocket_tx
        .sink_map_err(|err| anyhow::Error::new(err).context("Failed to send Websocket message"))
        .with(|event: super::BroadcastEvent| async move {
            rmp_serde::to_vec_named(&event)
                .map(axum::extract::ws::Message::Binary)
                .map_err(anyhow::Error::new)
        });

    tokio::pin!(websocket_tx);

    websocket_tx
        .send(rradio_messages::protocol_version_message())
        .await?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let commands = port_channels.commands_tx;

    // Handle incoming websocket messages
    wait_handle.spawn_task(
        async move {
            while let Some(message) = websocket_rx.next().await {
                let data = match message? {
                    axum::extract::ws::Message::Text(text) => text.into_bytes(),
                    axum::extract::ws::Message::Binary(binary) => binary,
                    axum::extract::ws::Message::Ping(_) => {
                        tracing::debug!("Ignoring ping messages");
                        continue;
                    }
                    axum::extract::ws::Message::Pong(_) => {
                        tracing::debug!("Ignoring pong messages");
                        continue;
                    }
                    axum::extract::ws::Message::Close(_) => break,
                };

                commands
                    .send(rmp_serde::from_read_ref(&data).context("Failed to decode Command")?)?;
            }

            drop(shutdown_tx);

            tracing::debug!("Close message received");

            Ok(())
        }
        .log_error(),
    );

    super::event_stream(
        port_channels.player_state_rx.clone(),
        port_channels.log_message_source.subscribe(),
    ) // Take each event
    .map(|event| Ok(event.into_broadcast_event())) // Convert each event into a [`BroadcastEvent`]
    .take_until(shutdown_rx) // Stop when the websocket is closed
    .take_until(port_channels.shutdown_signal.clone().wait()) // Stop when rradio closes
    .forward(&mut websocket_tx) // Send each event to the websocket
    .await?;

    websocket_tx.close().await?;

    drop(wait_handle);

    Ok(())
}

pub async fn run(port_channels: super::PortChannels, web_app_static_files: String) {
    let wait_group = crate::task::WaitGroup::new();
    let wait_handle = wait_group.clone_handle();

    let commands_tx = port_channels.commands_tx.clone();
    let ws_shutdown_signal = port_channels.shutdown_signal.clone();

    let app = axum::Router::new()
        .fallback(
            get_service(
                tower_http::services::ServeDir::new(web_app_static_files).not_found_service(
                    tower::service_fn(|request: axum::http::Request<_>| async move {
                        std::io::Result::Ok(format!("{} not found", request.uri()).into_response())
                    }),
                ),
            )
            .handle_error(|err: std::io::Error| async move {
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    err.to_string(),
                )
            }),
        )
        .route(
            "/command",
            post({
                let commands_tx = commands_tx.clone();
                |axum::Json(command): axum::Json<rradio_messages::Command>| async move {
                    commands_tx
                        .send(command)
                        .map_err(|tokio::sync::mpsc::error::SendError(_)| {
                            (
                                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                                "Failed to send command",
                            )
                        })
                }
            }),
        )
        .route(
            "/api",
            get(|ws: axum::extract::WebSocketUpgrade| async move {
                ws.on_upgrade(move |ws| {
                    handle_websocket_connection(port_channels.clone(), wait_handle, ws).log_error()
                })
            }),
        );

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

    let server = axum::Server::bind(&socket_addr.into()).serve(app.into_make_service());

    let server_addr = server.local_addr();

    let server = server.with_graceful_shutdown(ws_shutdown_signal.wait());

    tracing::info!("Listening on {}", server_addr);

    server
        .map(|result| result.context("Failed to run server"))
        .log_error()
        .await;

    tracing::debug!("Shutting down");

    wait_group.wait().await;

    tracing::debug!("Shut down");
}
