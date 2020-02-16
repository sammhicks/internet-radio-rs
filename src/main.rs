#![warn(clippy::pedantic)]

use std::io::stdout;

use anyhow::Result;
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

struct ChannelState {
    channel: channel::Channel,
    index: usize,
}

async fn process_commands(
    config: config::Config,
    pipeline: playbin::Playbin,
    mut commands: command::Receiver,
    events: event::Sender,
) -> Result<()> {
    let mut error_handler = error_handler::ErrorHandler::new(events.clone());

    let mut current_state = None;

    while let Some(command) = commands.recv().await {
        error_handler.handle(match command {
            Command::PlayPause => pipeline.play_pause(),
            Command::SetChannel(index) => channel::load(&config.channels_directory, index)
                .and_then(|new_channel| match new_channel.playlist.get(0) {
                    Some(entry) => {
                        current_state = Some(ChannelState {
                            channel: new_channel.clone(),
                            index: 0,
                        });
                        pipeline.set_url(&entry.url).and_then(|_| {
                            events
                                .send(event::Event::NewChannel(new_channel.clone()))
                                .map_err(anyhow::Error::new)
                        })
                    }
                    None => Err(anyhow::Error::msg("Empty Playlist")),
                }),
            Command::PreviousItem => {
                if let Some(current_state) = &mut current_state {
                    current_state.index = if current_state.index == 0 {
                        current_state.channel.playlist.len() - 1
                    } else {
                        current_state.index - 1
                    };

                    current_state
                        .channel
                        .playlist
                        .get(current_state.index)
                        .ok_or_else(|| anyhow::Error::msg("Failed to get playlist item"))
                        .and_then(|entry| pipeline.set_url(&entry.url))
                } else {
                    Ok(())
                }
            }
            Command::NextItem => {
                if let Some(current_state) = &mut current_state {
                    current_state.index += 1;
                    if current_state.index == current_state.channel.playlist.len() {
                        current_state.index = 0;
                    }

                    current_state
                        .channel
                        .playlist
                        .get(current_state.index)
                        .ok_or_else(|| anyhow::Error::msg("Failed to get playlist item"))
                        .and_then(|entry| pipeline.set_url(&entry.url))
                } else {
                    Ok(())
                }
            }
            Command::VolumeUp => pipeline.change_volume(config.volume_offset_percent),
            Command::VolumeDown => pipeline.change_volume(-config.volume_offset_percent),
        });
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
