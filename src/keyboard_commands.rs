use std::iter::FromIterator;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent};
use futures::stream::StreamExt;
use tokio::{
    sync::mpsc,
    time::{self, Duration},
};

use crate::message::Command;

struct RawMode {
    is_enabled: bool,
}

impl RawMode {
    fn new() -> Result<Self> {
        crossterm::terminal::enable_raw_mode()
            .map(|()| Self { is_enabled: true })
            .map_err(anyhow::Error::new)
    }

    fn disable(&mut self) -> Result<()> {
        if std::mem::replace(&mut self.is_enabled, false) {
            crossterm::terminal::disable_raw_mode().map_err(anyhow::Error::new)
        } else {
            Ok(())
        }
    }
}

impl std::ops::Drop for RawMode {
    fn drop(&mut self) {
        if let Some(err) = self.disable().err() {
            log::error!("Failed to disable raw mode: {:?}", err);
        }
    }
}

pub async fn run(
    commands: mpsc::UnboundedSender<Command>,
    timeout_duration: Duration,
) -> Result<()> {
    let mut raw_mode = RawMode::new()?;

    let mut keyboard_events = EventStream::new();

    let mut current_number_entry: Option<char> = None;

    loop {
        let previous_digit;
        let next_event;
        if let Some(digit) = current_number_entry.take() {
            if let Ok(event) = time::timeout(timeout_duration, keyboard_events.next()).await {
                previous_digit = Some(digit);
                next_event = event;
            } else {
                continue;
            }
        } else {
            previous_digit = None;
            next_event = keyboard_events.next().await;
        }

        let next_code = match next_event {
            Some(Ok(Event::Key(KeyEvent { code, .. }))) => code,
            Some(Ok(_)) => continue,
            Some(Err(err)) => anyhow::bail!(err),
            None => break,
        };

        let command = match next_code {
            KeyCode::Esc => break,
            KeyCode::Enter | KeyCode::Char(' ') => Command::PlayPause,
            KeyCode::Char('-') => Command::PreviousItem,
            KeyCode::Char('+') => Command::NextItem,
            KeyCode::Char('*') => Command::VolumeUp,
            KeyCode::Char('/') => Command::VolumeDown,
            KeyCode::Char(c) if c.is_ascii_digit() => {
                log::debug!("ASCII entry: {}", c);
                if let Some(previous_digit) = previous_digit {
                    Command::SetChannel(String::from_iter([previous_digit, c].iter()))
                } else {
                    current_number_entry = Some(c);
                    continue;
                }
            }
            code => {
                log::debug!("Unhandled key: {:?}", code);
                continue;
            }
        };

        commands.send(command)?;
    }

    raw_mode.disable()
}
