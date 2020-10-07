use anyhow::{Context, Result};
use futures::StreamExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::watch;

use crate::pipeline::PlayerState;

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
    mut player_state: watch::Receiver<PlayerState>,
) -> Result<()> {
    let mut guard = StreamGuard::new(stream);
    let guarded_stream = &mut guard.stream;

    let mut current_state = (*player_state.borrow()).clone();

    let init_state_str = format!("{:?}\r\n", super::state_to_diff(&current_state));
    guarded_stream.write_all(init_state_str.as_bytes()).await?;

    while let Some(new_state) = player_state.next().await {
        let diff = super::diff_player_state(&current_state, &new_state);
        let diff_state_str = format!("{:?}\r\n", diff);
        guarded_stream.write_all(diff_state_str.as_bytes()).await?;

        current_state = new_state;
    }

    guard.shutdown()?;

    Ok(())
}

pub async fn run(player_state: watch::Receiver<PlayerState>) -> Result<()> {
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
        )));
    }
}
