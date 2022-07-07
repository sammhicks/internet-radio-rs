use anyhow::Context;
use axum::{
    response::IntoResponse,
    routing::{get, get_service, post},
};
use futures::{FutureExt, SinkExt, StreamExt};
use tokio::sync::oneshot;

use rradio_messages::Event;

use crate::task::FailableFuture;

fn websocket_protocol() -> &'static str {
    rradio_messages::API_VERSION_HEADER.trim()
}

enum WebSocketUpgradeRejection {
    BadRequest(axum::extract::ws::rejection::WebSocketUpgradeRejection),
    NoProtocol,
    BadProtocol(axum::http::HeaderValue),
}

impl axum::response::IntoResponse for WebSocketUpgradeRejection {
    fn into_response(self) -> axum::response::Response {
        let code = axum::http::StatusCode::BAD_REQUEST;
        match self {
            WebSocketUpgradeRejection::BadRequest(rejection) => rejection.into_response(),
            WebSocketUpgradeRejection::NoProtocol => {
                (code, "websocket protocol not specified").into_response()
            }
            WebSocketUpgradeRejection::BadProtocol(protocol) => (
                code,
                format!(
                    "expected protocol {:?}, got protocol {:?}",
                    websocket_protocol(),
                    protocol
                ),
            )
                .into_response(),
        }
    }
}

struct WebSocketUpgrade(axum::extract::WebSocketUpgrade);

impl WebSocketUpgrade {
    fn on_upgrade<F, Fut>(self, callback: F) -> axum::response::Response
    where
        F: FnOnce(axum::extract::ws::WebSocket) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        self.0.on_upgrade(callback)
    }
}

#[axum::async_trait]
impl<B> axum::extract::FromRequest<B> for WebSocketUpgrade
where
    B: Send,
{
    type Rejection = WebSocketUpgradeRejection;

    async fn from_request(
        req: &mut axum::extract::RequestParts<B>,
    ) -> Result<Self, Self::Rejection> {
        let upgrade = axum::extract::WebSocketUpgrade::from_request(req)
            .await
            .map_err(WebSocketUpgradeRejection::BadRequest)?;

        let protocol = req
            .headers()
            .get(axum::http::header::SEC_WEBSOCKET_PROTOCOL)
            .ok_or(WebSocketUpgradeRejection::NoProtocol)?;

        let protocol_str = protocol
            .to_str()
            .map_err(|_| WebSocketUpgradeRejection::BadProtocol(protocol.clone()))?;

        if protocol_str == websocket_protocol() {
            Ok(Self(upgrade.protocols([websocket_protocol()])))
        } else {
            return Err(WebSocketUpgradeRejection::BadProtocol(protocol.clone()));
        }
    }
}

async fn handle_websocket_connection(
    port_channels: super::PortChannels,
    wait_handle: crate::task::WaitGroupHandle,
    websocket: axum::extract::ws::WebSocket,
) -> anyhow::Result<()> {
    let (websocket_tx, mut websocket_rx) = websocket.split();

    // Convert the websocket sink (i.e. of websocket [axum::extract::ws::Message]) into a sink of [`BroadcastEvent`]
    let websocket_tx = websocket_tx
        .sink_map_err(|err| anyhow::Error::new(err).context("Failed to send websocket message"))
        .with(|event: Event| async move {
            let mut buffer = Vec::new();
            event
                .encode(&mut buffer)
                .context("Failed to encode Event")?;

            Ok::<_, anyhow::Error>(axum::extract::ws::Message::Binary(buffer))
        });

    tokio::pin!(websocket_tx);

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    let commands = port_channels.commands_tx;

    // Handle incoming websocket messages
    wait_handle.spawn_task(
        async move {
            while let Some(message) = websocket_rx.next().await {
                let mut data = match message.context("Failed to recieve websocket message")? {
                    axum::extract::ws::Message::Text(text) => {
                        tracing::debug!("Ignoring Text message: {:?}", text);
                        continue;
                    }
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

                commands.send(
                    rradio_messages::Command::decode(&mut data)
                        .context("Failed to decode Command")?,
                )?;
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
    .map(Ok) // Convert each event into a [`BroadcastEvent`]
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
            get(|ws: WebSocketUpgrade| async move {
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
