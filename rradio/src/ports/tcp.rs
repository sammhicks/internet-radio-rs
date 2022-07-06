//! Common code for TCP ports

use anyhow::{Context, Result};
use futures::{Stream, StreamExt, TryStreamExt};
use tokio::{io::AsyncWriteExt, net::tcp, sync::oneshot};

use rradio_messages::{Command, Event};
use tracing::Instrument;

use crate::task::FailableFuture;

async fn send_messages<Events, Encode>(
    events: Events,
    encode_event: Encode,
    mut stream_tx: tcp::OwnedWriteHalf,
) -> Result<()>
where
    Events: Stream<Item = Event>,
    Encode: for<'a> Fn(&Event, &'a mut Vec<u8>) -> Result<&'a [u8]> + Send + Sync,
{
    let mut buffer = Vec::new();

    stream_tx
        .write_all(rradio_messages::API_VERSION_HEADER.as_bytes())
        .await?;

    buffer.clear();

    tokio::pin!(events);

    while let Some(event) = events.next().await {
        buffer.clear();

        stream_tx
            .write_all(encode_event(&event, &mut buffer)?)
            .await?;
    }

    stream_tx.shutdown().await?;

    Ok(())
}

pub async fn run<Encode, Decode, DecodeStream>(
    port_channels: super::PortChannels,
    port: u16,
    encode_event: Encode,
    decode_commands: Decode,
) where
    Encode: for<'a> Fn(&Event, &'a mut Vec<u8>) -> Result<&'a [u8]> + Send + Sync + Clone + 'static,
    Decode: FnOnce(tcp::OwnedReadHalf) -> DecodeStream + Send + Sync + Clone + 'static,
    DecodeStream: Stream<Item = Result<Command>> + Send + Sync + 'static,
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

        let connections = futures::stream::unfold(listener, |listener| async {
            let value = listener.accept().await;
            Some((value, listener))
        })
        .take_until(port_channels.shutdown_signal.clone().wait());

        tracing::info!("Listening on {:?}", local_addr);

        tokio::pin!(connections);

        while let Some((connection, remote_addr)) = connections.next().await.transpose()? {
            let _span = tracing::info_span!("tcp", %remote_addr).entered();
            tracing::info!("Connection");

            let (connection_rx, connection_tx) = connection.into_split();
            let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

            wait_group.spawn_task({
                let commands_rx = (decode_commands.clone())(connection_rx);
                let commands_tx = port_channels.commands_tx.clone();
                async move {
                    commands_rx
                        .try_for_each(|command| async {
                            commands_tx.send(command).context("Failed to send command")
                        })
                        .await?;

                    tracing::debug!("Disconnection");

                    drop(shutdown_tx);
                    Ok(())
                }
                .log_error()
            });

            wait_group.spawn_task({
                let events = super::event_stream(
                    port_channels.player_state_rx.clone(),
                    port_channels.log_message_source.subscribe(),
                )
                .take_until(shutdown_rx)
                .take_until(port_channels.shutdown_signal.clone().wait());

                let encode_event = encode_event.clone();
                async move {
                    send_messages(events, encode_event, connection_tx).await?;
                    tracing::debug!("Closing connection");
                    Ok(())
                }
                .log_error()
            });
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
