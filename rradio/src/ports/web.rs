use std::net::SocketAddr;

use anyhow::Context;
use axum::{
    extract::ConnectInfo,
    response::IntoResponse,
    routing::{get, get_service, post},
};
use futures::{SinkExt, StreamExt, TryStreamExt};
use tokio::sync::oneshot;

use rradio_messages::Event;
use tower::{Service, ServiceExt};

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

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

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

        drop(shutdown_tx);

        Ok(())
    });

    wait_handle.spawn_task(tracing::error_span!("forward_events"), async move {
        events_rx
            .map(Ok)
            .take_until(shutdown_rx) // Stop when the websocket is closed
            .forward(websocket_tx) // Send each event to the websocket
            .await?;

        tracing::debug!("Closing connection");

        Ok(())
    });

    Ok(())
}

pub async fn run(
    port_channels: super::PortChannels,
    web_app_static_files: String,
) -> anyhow::Result<()> {
    let wait_group = crate::task::WaitGroup::new();
    let wait_handle = wait_group.clone_handle();

    let commands_tx = port_channels.commands_tx.clone();
    let shutdown_signal = port_channels.shutdown_signal.clone();

    let app = axum::Router::new()
        .fallback_service(
            get_service(
                tower_http::services::ServeDir::new(web_app_static_files).not_found_service(
                    tower::service_fn(|request: axum::http::Request<_>| async move {
                        Ok(format!("{} not found", request.uri()).into_response())
                    }),
                ),
            )
            ,
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
            get(
                |ConnectInfo(remote_address): ConnectInfo<SocketAddr>, ws: WebSocketUpgrade| async move {
                    ws.on_upgrade(move |websocket| {
                        handle_websocket_connection(port_channels.clone(), wait_handle, websocket)
                            .log_error(tracing::error_span!("websocket_connection", %remote_address))
                    })
                },
            ),
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
    let server_addr = std::net::SocketAddr::from((addr, port));

    let listener = tokio::net::TcpListener::bind(server_addr)
        .await
        .with_context(|| format!("Failed to listen to {server_addr:?}"))?;

    tracing::info!(%server_addr, "Listening");

    let connections = futures::stream::try_unfold(listener, |listener| async move {
        anyhow::Ok(Some((
            listener
                .accept()
                .await
                .context("Failed to accept connection")?,
            listener,
        )))
    })
    .take_until(shutdown_signal.wait());

    tokio::pin!(connections);

    let mut make_service = app.into_make_service_with_connect_info::<SocketAddr>();

    while let Some((socket, remote_address)) = connections.try_next().await? {
        let service = make_service
            .call(remote_address)
            .await
            .unwrap_or_else(|err| match err {});

        wait_group.spawn_task(
            tracing::error_span!("connection", %remote_address),
            async move {
                hyper_util::server::conn::auto::Builder::new(hyper_util::rt::TokioExecutor::new())
                    .serve_connection_with_upgrades(
                        hyper_util::rt::TokioIo::new(socket),
                        hyper::service::service_fn(
                            move |request: axum::extract::Request<hyper::body::Incoming>| {
                                service.clone().oneshot(request)
                            },
                        ),
                    )
                    .await
                    .map_err(|err| anyhow::anyhow!("Failed to serve connection: {err}"))
            },
        );
    }

    tracing::debug!("Shutting down");

    // drop service to drop wait_handle above
    drop(make_service);
    wait_group.wait().await;

    tracing::debug!("Shut down");

    Ok(())
}
