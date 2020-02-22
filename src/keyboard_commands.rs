use std::iter::FromIterator;

use actix::prelude::*;
use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent};
use futures::stream::StreamExt;
use log::{debug, error};

use crate::message::Command;

#[derive(Debug)]
struct CurrentNumberEntry {
    previous_digit: char,
    timeout: SpawnHandle,
}

pub struct KeyboardCommands {
    command_handler: actix::Recipient<Command>,
    channel_timeout_duration: actix::clock::Duration,
    channel_timeout: Option<CurrentNumberEntry>,
}

impl KeyboardCommands {
    pub fn new(
        command_handler: actix::Recipient<Command>,
        channel_timeout_duration: actix::clock::Duration,
    ) -> Result<Self> {
        crossterm::terminal::enable_raw_mode()?;

        Ok(Self {
            command_handler,
            channel_timeout_duration,
            channel_timeout: None,
        })
    }

    fn send_command(&mut self, command: Command) {
        if self.command_handler.do_send(command).is_err() {
            System::current().stop();
        }
    }
}

impl Actor for KeyboardCommands {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        Self::add_stream(
            EventStream::new().filter_map(|event| {
                async move {
                    match event {
                        Ok(Event::Key(KeyEvent { code, .. })) => Some(Ok(code)),
                        Ok(..) => None,
                        Err(err) => Some(Err(err)),
                    }
                }
            }),
            ctx,
        );
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        if let Err(err) = crossterm::terminal::disable_raw_mode() {
            error!("Failed to disable raw mode: {:?}", err);
        }
    }
}

impl StreamHandler<crossterm::Result<KeyCode>> for KeyboardCommands {
    fn handle(&mut self, event: crossterm::Result<KeyCode>, ctx: &mut Self::Context) {
        let event = match event {
            Ok(event) => event,
            Err(err) => {
                error!("Crossterm error: {:?}", err);
                return;
            }
        };

        match event {
            KeyCode::Esc => actix::System::current().stop(),
            KeyCode::Char(' ') => self.send_command(Command::PlayPause),
            KeyCode::Char('-') => self.send_command(Command::PreviousItem),
            KeyCode::Char('+') => self.send_command(Command::NextItem),
            KeyCode::Char('*') => self.send_command(Command::VolumeUp),
            KeyCode::Char('/') => self.send_command(Command::VolumeDown),
            KeyCode::Char(c) if c.is_ascii_digit() => {
                debug!("ASCII entry: {}", c);
                if let Some(CurrentNumberEntry {
                    previous_digit,
                    timeout,
                }) = self.channel_timeout.take()
                {
                    ctx.cancel_future(timeout);
                    self.send_command(Command::SetChannel(String::from_iter(
                        [previous_digit, c].iter(),
                    )))
                } else {
                    self.channel_timeout = Some(CurrentNumberEntry {
                        previous_digit: c,
                        timeout: ctx.run_later(
                            self.channel_timeout_duration,
                            |actor: &mut Self, _ctx: &mut Self::Context| {
                                debug!("Channel entry timeout");
                                actor.channel_timeout = None
                            },
                        ),
                    });
                }
            }
            code => debug!("Unhandled key: {:?}", code),
        }
    }
}
