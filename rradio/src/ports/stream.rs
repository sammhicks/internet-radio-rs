use anyhow::{Context, Result};
use futures::{FutureExt, Stream, StreamExt};
use tokio::{
    io::{AsyncRead, AsyncWrite, AsyncWriteExt},
    sync::oneshot,
};

use rradio_messages::{Command, Event};

use crate::task::{FailableFuture, WaitGroup};

pub trait Splittable {
    type OwnedReadHalf: AsyncRead + Unpin + Send + 'static;
    type OwnedWriteHalf: AsyncWrite + Unpin + Send + 'static;

    fn into_split(self) -> (Self::OwnedReadHalf, Self::OwnedWriteHalf);
}

pub async fn forward_commands<Commands: Stream<Item = Result<Command>> + Send + Sync + 'static>(
    commands_tx: tokio::sync::mpsc::UnboundedSender<rradio_messages::Command>,
    commands_rx: Commands,
) {
    async move {
        tokio::pin!(commands_rx);

        while let Some(command) = commands_rx.next().await.transpose()? {
            commands_tx
                .send(command)
                .context("Failed to send command")?;
        }

        tracing::debug!("Disconnection");

        Ok(())
    }
    .log_error()
    .await;
}

pub async fn forward_events<
    EventEncoder: for<'a> Fn(&Event, &'a mut Vec<u8>) -> Result<&'a [u8]> + Send + Sync + Clone + 'static,
    Events: Stream<Item = rradio_messages::Event>,
    Connection: AsyncWrite + Unpin + Send + 'static,
>(
    encode_event: EventEncoder,
    events: Events,
    mut connection_tx: Connection,
) {
    async move {
        let mut buffer = Vec::new();

        connection_tx
            .write_all(rradio_messages::API_VERSION_HEADER.as_bytes())
            .await?;

        buffer.clear();

        tokio::pin!(events);

        while let Some(event) = events.next().await {
            buffer.clear();

            connection_tx
                .write_all(encode_event(&event, &mut buffer)?)
                .await
                .context("Failed to write encoded Event")?;
        }

        connection_tx.shutdown().await?;

        tracing::debug!("Closing connection");
        Ok(())
    }
    .log_error()
    .await;
}

pub fn handle_connection<S: Splittable, EventEncoder, CommandsDecoder, Commands>(
    connection: S,
    port_channels: &super::PortChannels,
    wait_group: &WaitGroup,
    encode_event: EventEncoder,
    decode_commands: CommandsDecoder,
) where
    EventEncoder:
        for<'a> Fn(&Event, &'a mut Vec<u8>) -> Result<&'a [u8]> + Send + Sync + Clone + 'static,
    CommandsDecoder: FnOnce(S::OwnedReadHalf) -> Commands + Send + Sync + Clone + 'static,
    Commands: Stream<Item = Result<Command>> + Send + Sync + 'static,
{
    let (connection_rx, connection_tx) = connection.into_split();
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    wait_group.spawn_task(
        forward_commands(
            port_channels.commands_tx.clone(),
            (decode_commands)(connection_rx),
        )
        .map(move |()| drop(shutdown_tx)),
    );

    wait_group.spawn_task(forward_events(
        encode_event,
        port_channels.event_stream().take_until(shutdown_rx),
        connection_tx,
    ));
}
