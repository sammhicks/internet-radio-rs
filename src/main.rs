#![warn(clippy::pedantic)]

use std::io::stdout;

use anyhow::Result;
use gstreamer::State;
use tokio::runtime::Runtime;

mod channel;
mod command;
mod config;
mod error_handler;
mod event;
mod event_logger;
mod keyboard_commands;
mod logger;
mod message_handler;
mod playbin;
mod playlist;
mod print_value;
mod raw_mode;
mod spawn_task;
mod tag;

use command::Command;
use logger::Logger;
use spawn_task::TaskSpawner;

async fn process_commands(
    config: config::Config,
    pipeline: playbin::Playbin,
    mut commands: command::Receiver,
    events: event::Sender,
) -> Result<()> {
    let mut error_handler = error_handler::ErrorHandler::new(events.clone());

    while let Some(command) = commands.recv().await {
        match command {
            Command::PlayPause => {
                if let Some(new_state) =
                    error_handler
                        .handle(pipeline.get_state())
                        .and_then(|current_state| match current_state {
                            State::Paused => Some(State::Playing),
                            State::Playing => Some(State::Paused),
                            _ => None,
                        })
                {
                    error_handler.handle(pipeline.set_state(new_state));
                }
            }
            Command::SetChannel(index) => {
                if let Some(new_channel) =
                    error_handler.handle(channel::load(&config.channels_directory, index))
                {
                    if error_handler
                        .handle(
                            new_channel
                                .playlist
                                .get(0)
                                .ok_or_else(|| anyhow::Error::msg("Empty Playlist"))
                                .and_then(|entry| pipeline.set_url(&entry.url)),
                        )
                        .is_some()
                    {
                        error_handler.handle(events.send(event::Event::NewChannel(new_channel)));
                    }
                }
            }
            Command::VolumeUp => {
                error_handler.handle(pipeline.change_volume(config.volume_offset_percent));
            }
            Command::VolumeDown => {
                error_handler.handle(pipeline.change_volume(-config.volume_offset_percent));
            }
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    log::set_boxed_logger(Logger::new(stdout()))?;
    log::set_max_level(log::LevelFilter::Trace);

    let config = config::load();

    log::set_max_level(config.log_level);

    gstreamer::init()?;
    let raw_mode = raw_mode::RawMode::new()?;

    let pipeline = playbin::Playbin::new()?;

    let (commands_tx, commands_rx) = tokio::sync::mpsc::unbounded_channel();
    let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel();

    let mut rt = Runtime::new()?;

    rt.spawn_named(
        "keyboard_commands",
        keyboard_commands::main(
            commands_tx,
            tokio::time::Duration::from_millis(config.input_timeout_ms),
        ),
    );

    rt.spawn_named(
        "message_handler",
        message_handler::main(pipeline.clone(), events_tx.clone()),
    );

    rt.spawn_named("event_logger", event_logger::main(events_rx));

    rt.block_on(process_commands(config, pipeline, commands_rx, events_tx))?;

    drop(raw_mode);

    Ok(())
}
