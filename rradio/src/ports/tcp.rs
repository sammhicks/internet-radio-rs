use anyhow::{Context, Result};
use futures::{Stream, StreamExt};
use tokio::{
    io::AsyncWriteExt,
    net::tcp,
    sync::{mpsc, oneshot},
};

use rradio_messages::Command;

use crate::task::FailableFuture;

use super::{BroadcastEvent, Event};

async fn send_messages<Events, Encode>(
    events: Events,
    encode_event: Encode,
    mut stream_tx: tcp::OwnedWriteHalf,
) -> Result<()>
where
    Events: Stream<Item = super::Event>,
    Encode: Fn(&BroadcastEvent) -> Result<Vec<u8>> + Send + Sync,
{
    stream_tx
        .write_all(
            encode_event(&BroadcastEvent::ProtocolVersion(
                rradio_messages::VERSION.into(),
            ))?
            .as_ref(),
        )
        .await?;

    tokio::pin!(events);

    while let Some(event) = events.next().await {
        match event {
            Event::StateUpdate(diff) => {
                stream_tx
                    .write_all(encode_event(&BroadcastEvent::PlayerStateChanged(diff))?.as_ref())
                    .await?;
            }
            Event::LogMessage(log_message) => {
                stream_tx
                    .write_all(encode_event(&BroadcastEvent::LogMessage(log_message))?.as_ref())
                    .await?;
            }
        }
    }

    Ok(())
}

async fn recieve_messages<Decode, DecodeStream>(
    stream_rx: tcp::OwnedReadHalf,
    decode_command: Decode,
    commands: mpsc::UnboundedSender<Command>,
) -> Result<()>
where
    Decode: FnOnce(tcp::OwnedReadHalf) -> DecodeStream + Send + Sync + 'static,
    DecodeStream: Stream<Item = Result<Command>> + Send + Sync + 'static,
{
    let decoded_messages = decode_command(stream_rx);

    tokio::pin!(decoded_messages);

    while let Some(next_command) = decoded_messages.next().await.transpose()? {
        commands.send(next_command)?;
    }

    Ok(())
}

pub async fn run<Encode, Decode, DecodeStream>(
    port_channels: super::PortChannels,
    current_module: &'static str,
    port: u16,
    encode_event: Encode,
    decode_command: Decode,
) where
    Encode: Fn(&BroadcastEvent) -> Result<Vec<u8>> + Send + Sync + Clone + 'static,
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

        log::info!(target: current_module, "Listening on {:?}", local_addr);

        tokio::pin!(connections);

        while let Some((connection, remote_addr)) = connections.next().await.transpose()? {
            log::info!(target: current_module, "Connection from {}", remote_addr);

            let (stream_rx, stream_tx) = connection.into_split();
            let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

            wait_group.spawn_task({
                let decode_command = decode_command.clone();
                let commands = port_channels.commands.clone();
                async move {
                    recieve_messages(stream_rx, decode_command, commands).await?;

                    log::debug!(target: current_module, "Disconnection from {}", remote_addr);

                    drop(shutdown_tx);
                    Ok(())
                }
                .context(remote_addr)
                .log_error(current_module)
            });

            wait_group.spawn_task({
                let events = super::event_stream(
                    port_channels.player_state.clone(),
                    port_channels.log_message_source.subscribe(),
                )
                .take_until(shutdown_rx)
                .take_until(port_channels.shutdown_signal.clone().wait());

                let encode_event = encode_event.clone();
                async move {
                    send_messages(events, encode_event, stream_tx).await?;
                    log::debug!(
                        target: current_module,
                        "Closing connection to {}",
                        remote_addr
                    );
                    Ok(())
                }
                .context(remote_addr)
                .log_error(current_module)
            });
        }

        log::debug!(target: current_module, "Shutting down");

        wait_group.wait().await;

        log::debug!(target: current_module, "Shut down");

        Ok(())
    }
    .log_error(current_module)
    .await
}
