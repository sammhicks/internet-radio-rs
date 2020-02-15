#![warn(clippy::pedantic)]

use std::io::stdout;

use anyhow::Result;
use gstreamer::State;

mod channel;
mod command;
mod config;
mod error_handler;
mod event;
mod event_logger;
mod keyboard_events;
mod logger;
mod message_handler;
mod playbin;
mod print_value;
mod raw_mode;
mod tag;

use command::Command;
use logger::Logger;

fn main() -> Result<()> {
    log::set_boxed_logger(Logger::new(stdout()))?;
    log::set_max_level(log::LevelFilter::Trace);

    let config = config::load();

    log::set_max_level(config.log_level);

    gstreamer::init()?;
    let raw_mode = raw_mode::RawMode::new()?;

    let playbin = playbin::Playbin::new()?;

    let (commands_tx, mut commands_rx) = tokio::sync::mpsc::unbounded_channel();
    let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel();

    let mut error_handler = error_handler::ErrorHandler::new(events_tx.clone());

    let mut rt = tokio::runtime::Runtime::new()?;

    rt.spawn(message_handler::main(playbin.clone(), events_tx.clone()));
    rt.spawn(event_logger::main(events_rx));
    rt.spawn(keyboard_events::main(
        commands_tx,
        tokio::time::Duration::from_millis(config.input_timeout_ms),
    ));

    while let Some(command) = rt.block_on(commands_rx.recv()) {
        match command {
            Command::PlayPause => {
                if let Some(new_state) =
                    error_handler
                        .handle(playbin.get_state())
                        .and_then(|current_state| match current_state {
                            State::Paused => Some(State::Playing),
                            State::Playing => Some(State::Paused),
                            _ => None,
                        })
                {
                    error_handler.handle(playbin.set_state(new_state));
                }
            }
            Command::SetChannel(index) => {
                let event = match config.station.iter().find(|c| c.index == index) {
                    Some(channel) => {
                        error_handler.handle(playbin.set_url(&channel.url));
                        event::Event::NewChannel(channel.clone())
                    }
                    None => event::Event::ChannelNotFound(index),
                };
                error_handler.handle(events_tx.send(event));
            }
        }
    }

    drop(events_tx);
    drop(raw_mode);

    Ok(())
}
