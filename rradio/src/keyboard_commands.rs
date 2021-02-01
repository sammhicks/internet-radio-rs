//! A task that reads commands from stdin (i.e the keyboard) and sends them through a given channel.
//! Radio station numbers are selected by the rapid entry of two digit codes.

use std::iter::FromIterator;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent};
use futures::StreamExt;
use tokio::{sync::mpsc, time};

use rradio_messages::Command;

use crate::task::FailableFuture;

/// `RawMode` is an RAII guard for the raw mode of stdin (and stdout).
///
/// Upon creation, raw mode is enabled for stdin and stdout.
///
/// When `RawMode` is dropped, raw mode is disabled for stdin and stdout.
///
/// # Raw Mode
/// When stdin is in raw mode, the input is unbuffered, so each key is send directly to the application, rather than buffering each line.
/// Also note that the shell does not intercept Ctrl+C.
struct RawMode {
    is_enabled: bool,
}

impl RawMode {
    /// Enables raw mode for stdin and stdout, and returns an RAII guard
    fn new() -> Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        Ok(Self { is_enabled: true })
    }

    /// Disable raw mode for stdin and stdout
    fn disable(&mut self) -> Result<()> {
        if std::mem::replace(&mut self.is_enabled, false) {
            crossterm::terminal::disable_raw_mode()?;
        }

        Ok(())
    }
}

impl std::ops::Drop for RawMode {
    /// Attempt to disable raw mode for stdin and stdout if not already disabled
    fn drop(&mut self) {
        if let Some(err) = self.disable().err() {
            log::error!("Failed to disable raw mode: {:#}", err);
        }
    }
}

/// Process keyboard input and send parsed commands through channel `commands`
pub async fn run(commands: mpsc::UnboundedSender<Command>, config: crate::config::Config) {
    async move {
        let mut raw_mode = RawMode::new()?;

        log::info!("Ready");

        let mut keyboard_events = EventStream::new();

        let mut current_number_entry: Option<char> = None;

        loop {
            let previous_digit;
            let keyboard_event;

            if let Some(digit) = current_number_entry.take() {
                // The user has recently entered a digit
                if let Ok(event) = time::timeout(config.input_timeout, keyboard_events.next()).await
                {
                    // The user pressed a key before the timeout
                    previous_digit = Some(digit);
                    keyboard_event = event;
                } else {
                    // The user didn't press a second key, so continue (discarding the previous key entry)
                    log::debug!("Station number input timeout");
                    continue;
                }
            } else {
                // The user has not recently entered a digit
                previous_digit = None;
                keyboard_event = keyboard_events.next().await;
            }

            let key_code = match keyboard_event {
                // Key event => extract key code
                Some(Ok(Event::Key(KeyEvent { code, .. }))) => code,
                // Other event => ignore and write value back to current_number_entry
                Some(Ok(_)) => {
                    current_number_entry = previous_digit;
                    continue;
                }
                // Error => return early with error
                Some(Err(err)) => anyhow::bail!(err),
                // No more events => break out of event loop
                None => break,
            };

            let command = match key_code {
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
                KeyCode::Char('d') => Command::DebugPipeline,
                code => {
                    log::debug!("Unhandled key: {:?}", code);
                    continue;
                }
            };

            commands.send(command)?;
        }

        log::debug!("Shutting down");

        raw_mode.disable()?;

        log::debug!("Shut down");

        Ok(())
    }
    .log_error(std::module_path!())
    .await
}
