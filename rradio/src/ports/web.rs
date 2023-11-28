use std::net::SocketAddr;

use anyhow::Context;
use axum::{
    extract::{FromRef, State},
    response::IntoResponse,
    routing::{get, get_service, post},
};
use futures_util::{SinkExt, StreamExt, TryStreamExt};
use tower::Service;

use rradio_messages::Event;

use crate::task::{FailableFuture, ShutdownSignal, WaitGroupHandle};

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
impl<S: Send + Sync> axum::extract::FromRequest<S> for WebSocketUpgrade {
    type Rejection = WebSocketUpgradeRejection;

    async fn from_request(
        req: axum::http::Request<axum::body::Body>,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let protocol = req
            .headers()
            .get(axum::http::header::SEC_WEBSOCKET_PROTOCOL)
            .ok_or(WebSocketUpgradeRejection::NoProtocol)?
            .clone();

        let protocol_str = String::from(
            protocol
                .to_str()
                .map_err(|_| WebSocketUpgradeRejection::BadProtocol(protocol.clone()))?,
        );

        if protocol_str == websocket_protocol() {
            Ok(Self(
                axum::extract::WebSocketUpgrade::from_request(req, state)
                    .await
                    .map_err(WebSocketUpgradeRejection::BadRequest)?
                    .protocols([websocket_protocol()]),
            ))
        } else {
            return Err(WebSocketUpgradeRejection::BadProtocol(protocol));
        }
    }
}

#[allow(clippy::unused_async)]
async fn handle_websocket_connection(
    port_channels: super::PortChannels,
    wait_handle: crate::task::WaitGroupHandle,
    websocket: axum::extract::ws::WebSocket,
) -> anyhow::Result<()> {
    tracing::debug!("Connection");

    let (websocket_tx, websocket_rx) = websocket.split();

    // Convert the websocket sink (i.e. of websocket [axum::extract::ws::Message]) into a sink of [`BroadcastEvent`]
    let websocket_tx = websocket_tx
        .sink_map_err(|err| anyhow::Error::msg(err).context("Failed to send websocket message"))
        .with(|event: Event| async move {
            let mut buffer = Vec::new();
            event
                .encode(&mut buffer)
                .context("Failed to encode Event")?;

            Ok::<_, anyhow::Error>(axum::extract::ws::Message::Binary(buffer))
        });

    let websocket_rx = websocket_rx
        .map_err(|err| anyhow::Error::msg(err).context("Failed to recieve websocket message"));

    let (shutdown_handle, shutdown_signal) = ShutdownSignal::new();

    let events_rx = port_channels.event_stream();
    let commands_tx = port_channels.commands_tx;

    // Handle incoming websocket messages
    wait_handle.spawn_task(tracing::error_span!("forward_commands"), async move {
        websocket_rx
            .try_filter_map(|message| async move {
                anyhow::Ok(match message {
                    axum::extract::ws::Message::Text(text) => {
                        tracing::debug!("Ignoring text message: {:?}", text);
                        None
                    }
                    axum::extract::ws::Message::Binary(mut buffer) => {
                        Some(rradio_messages::Command::decode(&mut buffer)?)
                    }
                    axum::extract::ws::Message::Ping(_) => {
                        tracing::debug!("Ignoring ping messages");
                        None
                    }
                    axum::extract::ws::Message::Pong(_) => {
                        tracing::debug!("Ignoring pong messages");
                        None
                    }
                    axum::extract::ws::Message::Close(_) => {
                        tracing::debug!("Close message received");
                        None
                    }
                })
            })
            .forward(super::CommandSink(commands_tx))
            .await?;

        shutdown_handle.signal_shutdown();

        Ok(())
    });

    wait_handle.spawn_task(tracing::error_span!("forward_events"), async move {
        events_rx
            .map(Ok)
            .take_until(shutdown_signal) // Stop when the websocket is closed
            .forward(websocket_tx) // Send each event to the websocket
            .await?;

        tracing::debug!("Closing connection");

        Ok(())
    });

    Ok(())
}

#[derive(Clone, FromRef)]
struct AppState {
    port_channels: super::PortChannels,
    wait_handle: WaitGroupHandle,
    remote_address: SocketAddr,
}

async fn handle_post_command(
    port_channels: State<super::PortChannels>,
    axum::Json(command): axum::Json<rradio_messages::Command>,
) -> impl IntoResponse {
    port_channels
        .commands_tx
        .send(command)
        .map_err(|tokio::sync::mpsc::error::SendError(_)| {
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to send command",
            )
        })
}

async fn handle_api(
    State(port_channels): State<super::PortChannels>,
    State(wait_handle): State<WaitGroupHandle>,
    upgrade: WebSocketUpgrade,
) -> impl IntoResponse {
    upgrade.on_upgrade(move |websocket| {
        handle_websocket_connection(port_channels, wait_handle, websocket)
            .log_error(tracing::error_span!("websocket_connection"))
    })
}

enum Never {}

async fn do_run(
    port_channels: super::PortChannels,
    web_app_static_files: String,
    wait_group: &crate::task::WaitGroup,
) -> anyhow::Result<Never> {
    let shutdown_signal = port_channels.shutdown_signal.clone();

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

    let app = axum::Router::new()
        .fallback_service(get_service(
            tower_http::services::ServeDir::new(web_app_static_files).not_found_service(
                axum::routing::any(
                    |uri: axum::http::Uri| async move { format!("{uri} not found") },
                ),
            ),
        ))
        .route("/command", post(handle_post_command))
        .route("/api", get(handle_api));

    let server_addr = std::net::SocketAddr::from((addr, port));

    let listener = tokio::net::TcpListener::bind(server_addr)
        .await
        .with_context(|| format!("Failed to listen to {server_addr:?}"))?;

    tracing::info!(%server_addr, "Listening");

    loop {
        let (socket, remote_address) = listener
            .accept()
            .await
            .context("Failed to accept connection")?;

        let shutdown_signal = shutdown_signal.clone();

        let port_channels = port_channels.clone();

        let app = app.clone().with_state(AppState {
            port_channels: port_channels.clone(),
            wait_handle: wait_group.clone_handle(),
            remote_address,
        });

        wait_group.spawn_task(
            tracing::error_span!("connection", %remote_address),
            async move {
                tracing::debug!("Connection");

                match futures_util::future::select(
                    shutdown_signal,
                    hyper::server::conn::http1::Builder::new()
                        .serve_connection(
                            hyper_util::rt::TokioIo::new(socket),
                            hyper::service::service_fn(move |request| app.clone().call(request)),
                        )
                        .with_upgrades(),
                )
                .await
                {
                    futures_util::future::Either::Left(((), mut connection)) => {
                        tracing::debug!("Shutting down");

                        // Gracefully shutdown
                        std::pin::Pin::new(&mut connection).graceful_shutdown();

                        // Wait for shutdown to finish
                        connection.await
                    }
                    futures_util::future::Either::Right((result, _)) => result,
                }
                .map_err(|err| anyhow::anyhow!("Failed to serve connection: {err}"))?;

                tracing::debug!("Disconnection");

                Ok(())
            },
        );
    }
}

pub async fn run(
    port_channels: super::PortChannels,
    web_app_static_files: String,
) -> anyhow::Result<()> {
    let wait_group = crate::task::WaitGroup::new();

    match futures_util::future::select(
        port_channels.shutdown_signal.clone(),
        std::pin::pin!(do_run(port_channels, web_app_static_files, &wait_group)),
    )
    .await
    {
        futures_util::future::Either::Left(((), _)) => (),
        futures_util::future::Either::Right((result, _)) => match result? {},
    }

    tracing::debug!("Shutting down");

    wait_group.wait().await;

    tracing::debug!("Shut down");

    Ok(())
}
