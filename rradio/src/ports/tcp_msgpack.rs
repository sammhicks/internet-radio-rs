use std::net::Shutdown;

use anyhow::{Context, Result};
use futures::StreamExt;
use tokio::{
    io::AsyncWriteExt,
    net::TcpStream,
    sync::{broadcast, mpsc, oneshot, watch},
};

use rradio_messages::{Command, LogMessage, Track};

use crate::{
    atomic_string::AtomicString,
    pipeline::{LogMessageSource, PlayerState},
};

use super::{Event, ShutdownSignal, WaitGroup, WaitGroupHandle};

type OutgoingMessage =
    rradio_messages::OutgoingMessage<&'static str, AtomicString, std::sync::Arc<[Track]>>;

fn extract_eof<T: std::fmt::Debug>(result: Result<T>) -> Result<Option<T>> {
    match result {
        Ok(success) => Ok(Some(success)),
        Err(err) => {
            if let Some(io_error) = err.downcast_ref::<std::io::Error>() {
                if let std::io::ErrorKind::UnexpectedEof = io_error.kind() {
                    return Ok(None);
                }
            }

            Err(err)
        }
    }
}

async fn read_command<Stream: tokio::io::AsyncRead + Unpin>(
    stream: &mut Stream,
) -> Result<Command> {
    use tokio::io::AsyncReadExt;

    let byte_count = stream.read_u16().await?;

    let mut buffer = vec![0; byte_count as usize];

    stream.read_exact(&mut buffer).await?;

    rmp_serde::from_slice(buffer.as_ref()).map_err(anyhow::Error::new)
}

async fn send_message<Stream: tokio::io::AsyncWrite + Unpin>(
    stream: &mut Stream,
    message: &OutgoingMessage,
) -> Result<()> {
    use std::convert::TryInto;

    let buffer = rmp_serde::to_vec(&message)?;

    stream.write_u16(buffer.len().try_into()?).await?;

    stream.write_all(&buffer).await?;

    Ok(())
}

async fn client_connected(
    stream: TcpStream,
    commands: mpsc::UnboundedSender<Command>,
    player_state: watch::Receiver<PlayerState>,
    log_message: broadcast::Receiver<LogMessage<AtomicString>>,
    shutdown_signal: ShutdownSignal,
    wait_handle: WaitGroupHandle,
) -> Result<()> {
    let remote_addr = stream.peer_addr()?;
    log::info!("Connection from {}", remote_addr);

    let (mut stream_rx, mut stream_tx) = stream.into_split();

    send_message(
        &mut stream_tx,
        &OutgoingMessage::ProtocolVersion(rradio_messages::VERSION),
    )
    .await?;

    let mut current_state = player_state.borrow().clone();

    send_message(
        &mut stream_tx,
        &OutgoingMessage::PlayerStateChanged(super::player_state_to_diff(&current_state)),
    )
    .await?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    wait_handle.spawn_task(async move {
        while let Some(next_command) = extract_eof(read_command(&mut stream_rx).await)
            .with_context(|| format!("Connection to {}", remote_addr))?
        {
            commands.send(next_command)?;
        }

        log::info!("TCP disconnection from {}", remote_addr);

        stream_rx.as_ref().shutdown(Shutdown::Read)?;

        drop(shutdown_tx);

        Ok(())
    });

    let player_state = player_state.map(super::Event::StateUpdate);
    let log_message = log_message.into_stream().filter_map(|msg| async {
        match msg {
            Ok(msg) => Some(Event::LogMessage(msg)),
            Err(_) => None,
        }
    });

    let events = futures::stream::select(player_state, log_message)
        .take_until(shutdown_rx)
        .take_until(shutdown_signal.wait());

    pin_utils::pin_mut!(events);

    while let Some(event) = events.next().await {
        match event {
            Event::StateUpdate(new_state) => {
                let state_diff = super::diff_player_state(&current_state, &new_state);
                current_state = new_state;
                send_message(
                    &mut stream_tx,
                    &OutgoingMessage::PlayerStateChanged(state_diff),
                )
                .await?;
            }
            Event::LogMessage(log_message) => {
                send_message(&mut stream_tx, &OutgoingMessage::LogMessage(log_message)).await?
            }
        }
    }

    log::debug!("No more TX to {}", remote_addr);

    stream_tx.as_ref().shutdown(Shutdown::Write)?;

    drop(wait_handle);

    Ok(())
}

pub async fn run(
    commands: mpsc::UnboundedSender<Command>,
    player_state: watch::Receiver<PlayerState>,
    log_message_source: LogMessageSource,
    shutdown_signal: ShutdownSignal,
) -> Result<()> {
    let addr = std::net::Ipv4Addr::LOCALHOST;
    let port = 8002;
    let socket_addr = (addr, port);

    let wait_group = WaitGroup::new();

    let listener = tokio::net::TcpListener::bind(socket_addr)
        .await
        .with_context(|| format!("Cannot listen to {:?}", socket_addr))?;

    let incoming = listener.take_until(shutdown_signal.clone().wait());

    pin_utils::pin_mut!(incoming);

    while let Some(connection) = incoming.next().await.transpose()? {
        wait_group.spawn_task(client_connected(
            connection,
            commands.clone(),
            player_state.clone(),
            log_message_source.subscribe(),
            shutdown_signal.clone(),
            wait_group.clone_handle(),
        ));
    }

    log::debug!("Shutting down");

    wait_group.wait().await;

    log::debug!("Shut down");

    Ok(())
}
