use anyhow::Result;
use futures_util::{Sink, Stream, StreamExt};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};

use rradio_messages::{Command, Event};

use crate::task::{ShutdownSignal, WaitGroup};

pub trait Splittable {
    type OwnedReadHalf: AsyncRead + Unpin + Send + 'static;
    type OwnedWriteHalf: AsyncWrite + Unpin + Send + 'static;

    fn into_split(self) -> (Self::OwnedReadHalf, Self::OwnedWriteHalf);
}

pub fn handle_connection<S: Splittable, EventsEncoder, Events, CommandsDecoder, Commands>(
    connection: S,
    port_channels: &super::PortChannels,
    wait_group: &WaitGroup,
    encode_events: EventsEncoder,
    decode_commands: CommandsDecoder,
) where
    EventsEncoder: FnOnce(S::OwnedWriteHalf) -> Events + Send + 'static,
    Events: Sink<Event, Error = anyhow::Error> + Send + 'static,
    CommandsDecoder: FnOnce(S::OwnedReadHalf) -> Commands + Send + 'static,
    Commands: Stream<Item = Result<Command>> + Send + 'static,
{
    let (connection_rx, mut connection_tx) = connection.into_split();
    let (shutdown_handle, shutdown_signal) = ShutdownSignal::new();

    wait_group.spawn_task(tracing::error_span!("forward_commands"), {
        let commands_tx = port_channels.commands_tx.clone();
        let commands_rx = (decode_commands)(connection_rx);
        async move {
            commands_rx.forward(super::CommandSink(commands_tx)).await?;

            tracing::debug!("Disconnection");

            shutdown_handle.signal_shutdown();

            Ok(())
        }
    });

    wait_group.spawn_task(tracing::error_span!("forward_events"), {
        let events = port_channels.event_stream().take_until(shutdown_signal);

        async move {
            connection_tx
                .write_all(rradio_messages::API_VERSION_HEADER.as_bytes())
                .await?;

            events
                .map(Ok)
                .forward((encode_events)(connection_tx))
                .await?;

            tracing::debug!("Closing connection");
            Ok(())
        }
    });
}
