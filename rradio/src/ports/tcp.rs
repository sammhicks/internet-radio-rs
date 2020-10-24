use std::net::Shutdown;

use anyhow::{Context, Result};
use futures::{Stream, StreamExt};
use tokio::{
    io::AsyncWriteExt,
    net::tcp,
    sync::{mpsc, oneshot},
};

use rradio_messages::Command;

use crate::{log_error::CanAttachContext, pipeline::PlayerState};

use super::{BroadcastEvent, Event, WaitGroup};

async fn send_messages<Events, Encode>(
    initial_state: PlayerState,
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
            encode_event(&BroadcastEvent::ProtocolVersion(rradio_messages::VERSION))?.as_ref(),
        )
        .await?;

    let mut current_state = initial_state;

    stream_tx
        .write_all(
            encode_event(&BroadcastEvent::PlayerStateChanged(
                super::player_state_to_diff(&current_state),
            ))?
            .as_ref(),
        )
        .await?;

    pin_utils::pin_mut!(events);

    while let Some(event) = events.next().await {
        match event {
            Event::StateUpdate(new_state) => {
                let state_diff = super::diff_player_state(&current_state, &new_state);
                current_state = new_state;
                stream_tx
                    .write_all(
                        encode_event(&BroadcastEvent::PlayerStateChanged(state_diff))?.as_ref(),
                    )
                    .await?;
            }
            Event::LogMessage(log_message) => {
                stream_tx
                    .write_all(encode_event(&BroadcastEvent::LogMessage(log_message))?.as_ref())
                    .await?;
            }
        }
    }

    stream_tx.as_ref().shutdown(Shutdown::Write)?;

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

    pin_utils::pin_mut!(decoded_messages);

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
) -> Result<()>
where
    Encode: Fn(&BroadcastEvent) -> Result<Vec<u8>> + Send + Sync + Clone + 'static,
    Decode: FnOnce(tcp::OwnedReadHalf) -> DecodeStream + Send + Sync + Clone + 'static,
    DecodeStream: Stream<Item = Result<Command>> + Send + Sync + 'static,
{
    let addr = if cfg!(feature = "production-server") {
        std::net::Ipv4Addr::UNSPECIFIED
    } else {
        std::net::Ipv4Addr::LOCALHOST
    };

    let socket_addr = (addr, port);

    let wait_group = WaitGroup::new();

    let listener = tokio::net::TcpListener::bind(socket_addr)
        .await
        .with_context(|| format!("Cannot listen to {:?}", socket_addr))?;

    let local_addr = listener.local_addr()?;

    let connections = super::connection_stream::ConnectionStream(listener)
        .take_until(port_channels.shutdown_signal.clone().wait());

    log::info!(target: current_module, "Listening on {:?}", local_addr);

    pin_utils::pin_mut!(connections);

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
        });

        wait_group.spawn_task({
            let initial_state = port_channels.player_state.borrow().clone();

            let player_state = port_channels
                .player_state
                .clone()
                .map(super::Event::StateUpdate);
            let log_message_source = port_channels
                .log_message_source
                .subscribe()
                .into_stream()
                .filter_map(|msg| async {
                    match msg {
                        Ok(msg) => Some(Event::LogMessage(msg)),
                        Err(_) => None,
                    }
                });

            let events = futures::stream::select(player_state, log_message_source)
                .take_until(shutdown_rx)
                .take_until(port_channels.shutdown_signal.clone().wait());

            let encode_event = encode_event.clone();
            async move {
                send_messages(initial_state, events, encode_event, stream_tx).await?;
                log::debug!(
                    target: current_module,
                    "Closing connection to {}",
                    remote_addr
                );
                Ok(())
            }
            .context(remote_addr)
        });
    }

    log::debug!(target: current_module, "Shutting down");

    wait_group.wait().await;

    log::debug!(target: current_module, "Shut down");

    Ok(())
}
