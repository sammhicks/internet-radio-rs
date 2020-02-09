use std::iter::FromIterator;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent};
use futures::{
    future::{select, Either},
    StreamExt,
};
use tokio::time::{delay_until, Duration, Instant};

use crate::channel::ChannelIndex;
use crate::command::{Command, CommandSender};

#[derive(Clone, Copy, Debug)]
struct CurrentNumberEntry {
    previous_digit: char,
    timeout: Instant,
}

pub async fn main(channel: CommandSender, station_index_timeout_duration: Duration) -> Result<()> {
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
                    if channel.send(Command::ChannelCancelled).is_err() {
                        break;
                    }

                    station_index_timeout = None;
                    continue;
                }
            },
            None => (events.next().await, None),
        };

        let event = match event {
            Some(event) => event?,
            None => break,
        };

        match event {
            Event::Key(KeyEvent {
                code: KeyCode::Esc,
                modifiers: _,
            }) => break,
            Event::Key(KeyEvent {
                code: KeyCode::Char(' '),
                modifiers: _,
            }) => {
                if channel.send(Command::PlayPause).is_err() {
                    break;
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char(c),
                modifiers: _,
            }) if c.is_ascii_digit() => {
                station_index_timeout = match current_digit {
                    Some(current_digit) => {
                        let station_index_str = String::from_iter([current_digit, c].into_iter());

                        let station_index =
                            ChannelIndex::from_str_radix(&station_index_str, 10).unwrap();

                        if channel.send(Command::SetChannel(station_index)).is_err() {
                            break;
                        }

                        None
                    }
                    None => {
                        if channel
                            .send(Command::PartialChannel(
                                c.to_digit(10).unwrap() as ChannelIndex
                            ))
                            .is_err()
                        {
                            break;
                        }
                        Some(CurrentNumberEntry {
                            previous_digit: c,
                            timeout: Instant::now() + station_index_timeout_duration,
                        })
                    }
                };
            }
            e => eprintln!("Unhandled key: {:?}", e),
        }
    }

    Ok(())
}
