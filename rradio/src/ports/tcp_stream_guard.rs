use std::net::Shutdown;

use anyhow::{Context, Result};
use tokio::net::{
    tcp::{OwnedReadHalf, OwnedWriteHalf},
    TcpStream,
};

pub trait GuardableStream {
    fn shutdown(&self) -> anyhow::Result<()>;
}

impl GuardableStream for TcpStream {
    fn shutdown(&self) -> anyhow::Result<()> {
        self.shutdown(Shutdown::Both)
            .context("Could not shutdown TCP stream")
    }
}

impl GuardableStream for OwnedReadHalf {
    fn shutdown(&self) -> anyhow::Result<()> {
        self.as_ref()
            .shutdown(Shutdown::Read)
            .context("Could not shutdown TCP stream")
    }
}

impl GuardableStream for OwnedWriteHalf {
    fn shutdown(&self) -> anyhow::Result<()> {
        self.as_ref()
            .shutdown(Shutdown::Write)
            .context("Could not shutdown TCP stream")
    }
}

pub struct StreamGuard<Stream: GuardableStream> {
    stream: Stream,
    is_shutdown: bool,
}

impl<Stream: GuardableStream> StreamGuard<Stream> {
    pub fn new(stream: Stream) -> Self {
        Self {
            stream,
            is_shutdown: false,
        }
    }

    pub fn shutdown(&mut self) -> Result<()> {
        if std::mem::replace(&mut self.is_shutdown, true) {
            Ok(())
        } else {
            self.stream.shutdown()
        }
    }
}

impl<Stream: GuardableStream> AsRef<Stream> for StreamGuard<Stream> {
    fn as_ref(&self) -> &Stream {
        &self.stream
    }
}

impl<Stream: GuardableStream> AsMut<Stream> for StreamGuard<Stream> {
    fn as_mut(&mut self) -> &mut Stream {
        &mut self.stream
    }
}

impl<Stream: GuardableStream> std::ops::Drop for StreamGuard<Stream> {
    fn drop(&mut self) {
        if let Err(err) = self.shutdown() {
            log::error!("{:?}", err)
        }
    }
}
