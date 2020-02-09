#![allow(dead_code)]
use anyhow::Result;
use gstreamer::State;

mod channel;
mod command;
mod error_handler;
mod event;
mod keyboard_events;
mod message_handler;
mod playbin;
mod print_value;
mod tag;

use command::Command;

fn default_input_timeout_ms() -> u64 {
    2000
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Config {
    #[serde(rename = "Input Timeout ms", default = "default_input_timeout_ms")]
    input_timeout_ms: u64,
    #[serde(rename = "Station")]
    station: Vec<channel::Channel>,
}

fn load_config() -> Result<Config> {
    let file = std::fs::read_to_string("config.toml")?;
    Ok(toml::from_str(&file)?)
}

async fn print_events(mut channel: event::EventReciever) {
    while let Some(event) = channel.recv().await {
        println!("Event: {:?}", event);
    }
    println!("print_events done");
}

fn main() -> Result<()> {
    let config = load_config().unwrap_or_else(|err| {
        eprintln!("Failed to load config file: {}", err);
        Config::default()
    });

    gstreamer::init()?;

    let playbin = playbin::Playbin::new()?;
    let bus = playbin.get_bus()?;

    let (commands_tx, mut commands_rx) = tokio::sync::mpsc::unbounded_channel();
    let (events_tx, events_rx) = tokio::sync::mpsc::unbounded_channel();

    let mut error_handler = error_handler::ErrorHandler::new(events_tx.clone());

    let mut rt = tokio::runtime::Runtime::new()?;

    rt.spawn(message_handler::main(bus, events_tx.clone()));
    rt.spawn(print_events(events_rx));
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
                        event::Event::NewChannel(index)
                    }
                    None => event::Event::ChannelNotFound(index),
                };
                error_handler.handle(events_tx.send(event));
            }
        }
    }

    drop(events_tx);

    let keyboard_result = rt.block_on(keyboard_task)?;

    keyboard_result
}
