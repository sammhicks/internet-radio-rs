//! Common code for TCP ports

use anyhow::{Context, Result};
use futures::{Stream, StreamExt};
use tokio::net::tcp;

use rradio_messages::{Command, Event};
use tracing::Instrument;

use crate::task::FailableFuture;

impl super::stream::Splittable for tokio::net::TcpStream {
    type OwnedReadHalf = tcp::OwnedReadHalf;
    type OwnedWriteHalf = tcp::OwnedWriteHalf;

    fn into_split(self) -> (Self::OwnedReadHalf, Self::OwnedWriteHalf) {
        tokio::net::TcpStream::into_split(self)
    }
}

pub async fn run<EventEncoder, CommandsDecoder, Commands>(
    port_channels: super::PortChannels,
    port: u16,
    encode_event: EventEncoder,
    decode_commands: CommandsDecoder,
) where
    EventEncoder:
        for<'a> Fn(&Event, &'a mut Vec<u8>) -> Result<&'a [u8]> + Send + Sync + Clone + 'static,
    CommandsDecoder: FnOnce(tcp::OwnedReadHalf) -> Commands + Send + Sync + Clone + 'static,
    Commands: Stream<Item = Result<Command>> + Send + Sync + 'static,
{
    async move {
        let addr = if cfg!(feature = "production-server") {
            std::net::Ipv4Addr::UNSPECIFIED
        } else {
            std::net::Ipv4Addr::LOCALHOST
        };

        let socket_addr = (addr, port);

        let wait_group = crate::task::WaitGroup::new();

        let listener = tokio::net::TcpListener::bind(socket_addr)
            .await
            .with_context(|| format!("Failed to listen to {:?}", socket_addr))?;

        let local_addr = listener
            .local_addr()
            .context("Failed to get local address")?;

        tracing::info!("Listening on {:?}", local_addr);

        let connections = futures::stream::unfold(listener, |listener| async {
            let value = listener.accept().await;
            Some((value, listener))
        })
        .take_until(port_channels.shutdown_signal.clone().wait());

        tokio::pin!(connections);

        while let Some((connection, remote_addr)) = connections.next().await.transpose()? {
            let _span = tracing::info_span!("tcp", %remote_addr).entered();
            tracing::info!("Connection");

            super::stream::handle_connection(
                connection,
                &port_channels,
                &wait_group,
                encode_event.clone(),
                decode_commands.clone(),
            );
        }

        tracing::debug!("Shutting down");

        wait_group.wait().await;

        tracing::debug!("Shut down");

        Ok(())
    }
    .log_error()
    .instrument(tracing::Span::current())
    .await;
}
