use anyhow::{Context, Result};
use tokio::{
    io::AsyncWriteExt,
    net::TcpStream,
    stream::StreamExt,
    sync::{broadcast, watch},
};

use rradio_messages::LogMessage;

use crate::{
    atomic_string::AtomicString,
    pipeline::{LogMessageSource, PlayerState},
};

use super::Message;

struct StreamGuard {
    stream: TcpStream,
    is_shutdown: bool,
}

impl StreamGuard {
    fn new(stream: TcpStream) -> Self {
        Self {
            stream,
            is_shutdown: false,
        }
    }

    fn shutdown(&mut self) -> Result<()> {
        if std::mem::replace(&mut self.is_shutdown, true) {
            Ok(())
        } else {
            self.stream
                .shutdown(std::net::Shutdown::Both)
                .context("Could not shutdown tcp stream")
        }
    }
}

impl std::ops::Drop for StreamGuard {
    fn drop(&mut self) {
        if let Err(err) = self.shutdown() {
            log::error!("{:?}", err)
        }
    }
}

async fn client_connected(
    stream: TcpStream,
    player_state: watch::Receiver<PlayerState>,
    log_message: broadcast::Receiver<LogMessage<AtomicString>>,
) -> Result<()> {
    let mut guard = StreamGuard::new(stream);
    let guarded_stream = &mut guard.stream;

    let mut current_state = (*player_state.borrow()).clone();

    let init_state_str = format!("{:?}\r\n", super::state_to_diff(&current_state));
    guarded_stream.write_all(init_state_str.as_bytes()).await?;

    let player_state = player_state.map(Message::StateUpdate);
    let log_message = log_message.into_stream().filter_map(|msg| match msg {
        Ok(msg) => Some(Message::LogMessage(msg)),
        Err(_) => None,
    });

    pin_utils::pin_mut!(player_state);
    pin_utils::pin_mut!(log_message);

    let mut messages = player_state.merge(log_message);

    while let Some(new_message) = messages.next().await {
        let message_to_send = match new_message {
            Message::StateUpdate(new_state) => {
                let diff = super::diff_player_state(&current_state, &new_state);
                let diff_state_str = format!("{:?}\r\n", diff);

                current_state = new_state;

                diff_state_str
            }
            Message::LogMessage(message) => format!("{:?}\r\n", message),
        };
        guarded_stream.write_all(message_to_send.as_bytes()).await?;
    }

    guard.shutdown()?;

    Ok(())
}

pub async fn run(
    player_state: watch::Receiver<PlayerState>,
    log_message_source: LogMessageSource,
) -> Result<()> {
    let addr = std::net::Ipv4Addr::LOCALHOST;
    let port = 8080;
    let socket_addr = (addr, port);

    let mut listener = tokio::net::TcpListener::bind(socket_addr)
        .await
        .with_context(|| format!("Cannot listen to {:?}", socket_addr))?;

    loop {
        let (stream, addr) = listener.accept().await?;
        log::info!("TCP connection from {}", addr);
        tokio::spawn(crate::log_error::log_error(client_connected(
            stream,
            player_state.clone(),
            log_message_source.subscribe(),
        )));
    }
}
