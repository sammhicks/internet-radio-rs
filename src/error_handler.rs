use anyhow::{Error, Result};
use log::error;

use crate::event::{Event, Sender};

pub struct ErrorHandler {
    channel: Sender,
}

impl ErrorHandler {
    pub const fn new(channel: Sender) -> Self {
        Self { channel }
    }

    pub fn handle<T, E: std::convert::Into<Error>>(&mut self, result: Result<T, E>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(err) => {
                let err: Error = err.into();
                if self.channel.send(Event::Error(err.to_string())).is_err() {
                    error!("Failed to send error: {}", err);
                }
                None
            }
        }
    }
}
