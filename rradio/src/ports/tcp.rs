//! Common code for TCP ports

use anyhow::{Context, Result};
use futures::{Sink, Stream, StreamExt, TryStreamExt};
use tokio::net::tcp;

use rradio_messages::{Command, Event};
use tracing::Instrument;

impl super::stream::Splittable for tokio::net::TcpStream {
    type OwnedReadHalf = tcp::OwnedReadHalf;
    type OwnedWriteHalf = tcp::OwnedWriteHalf;

    fn into_split(self) -> (Self::OwnedReadHalf, Self::OwnedWriteHalf) {
        tokio::net::TcpStream::into_split(self)
    }
}

pub async fn run<EventsEncoder, Events, CommandsDecoder, Commands>(
    port_channels: super::PortChannels,
    port: u16,
    encode_events: EventsEncoder,
    decode_commands: CommandsDecoder,
) -> anyhow::Result<()>
where
    EventsEncoder: FnOnce(tcp::OwnedWriteHalf) -> Events + Send + Sync + Clone + 'static,
    Events: Sink<Event, Error = anyhow::Error> + Send + Sync + 'static,
    CommandsDecoder: FnOnce(tcp::OwnedReadHalf) -> Commands + Send + Sync + Clone + 'static,
    Commands: Stream<Item = Result<Command>> + Send + Sync + 'static,
{
    async move {
        let addr = if cfg!(feature = "production-server") {
            std::net::Ipv4Addr::UNSPECIFIED
        } else {
            std::net::Ipv4Addr::LOCALHOST
        };

        let socket_addr = std::net::SocketAddr::from((addr, port));

        let wait_group = crate::task::WaitGroup::new();

        let listener = tokio::net::TcpListener::bind(socket_addr)
            .await
            .with_context(|| format!("Failed to listen to {socket_addr:?}"))?;

        tracing::info!(%socket_addr, "Listening");

        let connections = futures::stream::try_unfold(listener, |listener| async {
            anyhow::Ok(Some((listener.accept().await?, listener)))
        })
        .take_until(port_channels.shutdown_signal.clone().wait());

        tokio::pin!(connections);

        while let Some((connection, remote_addr)) = connections.try_next().await? {
            let _span = tracing::error_span!("connection", %remote_addr).entered();
            tracing::debug!("Connection");

            super::stream::handle_connection(
                connection,
                &port_channels,
                &wait_group,
                encode_events.clone(),
                decode_commands.clone(),
            );
        }

        tracing::debug!("Shutting down");

        wait_group.wait().await;

        tracing::debug!("Shut down");

        Ok(())
    }
    .instrument(tracing::error_span!("tcp"))
    .await
}
