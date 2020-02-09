use anyhow::{Error, Result};
use log::error;

use crate::event::{Event, EventSender};

pub struct ErrorHandler {
    channel: EventSender,
}

impl ErrorHandler {
    pub fn new(channel: EventSender) -> Self {
        ErrorHandler { channel }
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
