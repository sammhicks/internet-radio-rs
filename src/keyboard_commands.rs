use std::iter::FromIterator;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent};
use futures::{
    future::{select, Either},
    StreamExt,
};
use log::debug;
use tokio::time::{delay_until, Duration, Instant};

use crate::command::{self, Command};

#[derive(Clone, Copy, Debug)]
struct CurrentNumberEntry {
    previous_digit: char,
    timeout: Instant,
}

pub async fn main(
    channel: command::Sender,
    station_index_timeout_duration: Duration,
) -> Result<()> {
    let mut events = EventStream::new();

    let mut station_index_timeout = None;

    loop {
        let (event, current_digit) = match station_index_timeout {
            Some(CurrentNumberEntry {
                previous_digit,
                timeout,
            }) => match select(events.next(), delay_until(timeout)).await {
                Either::Left((event, _)) => (event, Some(previous_digit)),
                Either::Right(_) => {
                    debug!("Channel entry cancelled");

                    station_index_timeout = None;
                    continue;
                }
            },
            None => (events.next().await, None),
        };

        match event.ok_or_else(|| anyhow::Error::msg("Keyboard event closed"))?? {
            Event::Key(KeyEvent {
                code: KeyCode::Esc, ..
            }) => return Ok(()),
            Event::Key(KeyEvent {
                code: KeyCode::Char(' '),
                ..
            }) => {
                channel.send(Command::PlayPause)?;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('-'),
                ..
            }) => {
                channel.send(Command::PreviousItem)?;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('+'),
                ..
            }) => {
                channel.send(Command::NextItem)?;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('*'),
                ..
            }) => {
                channel.send(Command::VolumeUp)?;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('/'),
                ..
            }) => {
                channel.send(Command::VolumeDown)?;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char(c),
                ..
            }) if c.is_ascii_digit() => {
                debug!("ASCII entry: {}", c);
                station_index_timeout = match current_digit {
                    Some(current_digit) => {
                        let station_index = String::from_iter([current_digit, c].iter());

                        channel.send(Command::SetChannel(station_index))?;

                        None
                    }
                    None => Some(CurrentNumberEntry {
                        previous_digit: c,
                        timeout: Instant::now() + station_index_timeout_duration,
                    }),
                };
            }
            e => debug!("Unhandled key: {:?}", e),
        }
    }
}
