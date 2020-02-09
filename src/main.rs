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

    let config = config::load_config();

    log::set_max_level(log::LevelFilter::Info);

    gstreamer::init()?;
    let raw_mode = raw_mode::RawMode::new()?;

    let playbin = playbin::Playbin::new()?;
    let bus = playbin.get_bus()?;

    let (commands_tx, mut commands_rx) = tokio::sync::mpsc::unbounded_channel();
    let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel();

    let mut error_handler = error_handler::ErrorHandler::new(events_tx.clone());

    let mut rt = tokio::runtime::Runtime::new()?;

    rt.spawn(message_handler::main(bus, events_tx.clone()));
    rt.spawn(event_logger::main(events_rx));
    let keyboard_task = rt.spawn(keyboard_events::main(
        commands_tx,
        tokio::time::Duration::from_millis(config.input_timeout_ms),
    ));

    let mut is_playing = false;

    while let Some(command) = rt.block_on(commands_rx.recv()) {
        match command {
            Command::PlayPause => {
                error_handler.handle(playbin.set_state(if is_playing {
                    State::Paused
                } else {
                    State::Playing
                }));
                is_playing = !is_playing;
            }
            Command::PartialChannel(c) => {
                error_handler.handle(events_tx.send(event::Event::PartialChannel(c)));
            }
            Command::ChannelCancelled => {
                error_handler.handle(events_tx.send(event::Event::ChannelCancelled));
            }
            Command::SetChannel(index) => {
                let event = match config.station.iter().find(|c| c.index == index) {
                    Some(channel) => {
                        error_handler.handle(
                            playbin
                                .set_url(&channel.url)
                                .and_then(|()| playbin.set_state(State::Playing))
                                .map(|()| {
                                    is_playing = true;
                                }),
                        );
                        event::Event::NewChannel(channel.clone())
                    }
                    None => event::Event::ChannelNotFound(index),
                };
                error_handler.handle(events_tx.send(event));
            }
        }
    }

    drop(events_tx);

    let keyboard_result = rt.block_on(keyboard_task)?;

    drop(raw_mode);

    keyboard_result
}
