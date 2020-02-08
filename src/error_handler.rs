use anyhow::Result;

use super::event::{Event, EventSender};

pub struct ErrorHandler {
    channel: EventSender,
}

impl ErrorHandler {
    pub fn new(channel: EventSender) -> Self {
        ErrorHandler { channel }
    }

    pub fn handle<T>(&mut self, result: Result<T>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(err) => {
                if let Err(_) = self.channel.send(Event::Error(err.to_string())) {
                    eprintln!("Failed to send error: {}", err);
                }
                None
            }
        }
    }
}
