#![warn(clippy::pedantic)]

use anyhow::Result;
use futures::FutureExt;
use tokio::sync::mpsc;

mod channel;
mod config;
mod keyboard_commands;
mod message;
mod pipeline;
mod playlist;
mod tag;
mod web_interface;

#[tokio::main]
async fn main() -> Result<()> {
    let mut logger = flexi_logger::Logger::with_str("error")
        .format(log_format)
        .start()?;

    let config = config::load();

    logger.parse_new_spec(&config.log_level);

    gstreamer::init()?;

    let (commands_tx, commands_rx) = mpsc::unbounded_channel();

    let keyboard_commands_task = keyboard_commands::run(commands_tx.clone(), config.input_timeout);
    let (pipeline_task, player_state_rx) = pipeline::run(config, commands_rx)?;
    let web_interface_task = web_interface::run(commands_tx.clone(), player_state_rx);

    futures::future::select_all(vec![
        keyboard_commands_task.boxed(),
        pipeline_task.boxed(),
        web_interface_task.boxed(),
    ])
    .await
    .0
    .map(|()| {
        logger.shutdown();
    })
}

fn log_format(
    w: &mut dyn std::io::Write,
    _now: &mut flexi_logger::DeferredNow,
    record: &log::Record,
) -> Result<(), std::io::Error> {
    use crossterm::style::{style, Attribute, Color};
    use log::Level;

    let color = match record.level() {
        Level::Trace => Color::Magenta,
        Level::Debug => Color::Blue,
        Level::Info => Color::Green,
        Level::Warn => Color::Yellow,
        Level::Error => Color::Red,
    };

    let level = style(record.level()).with(color);

    let target = record.target();

    let args = match record.level() {
        Level::Trace => style(record.args()).with(Color::DarkGrey),
        Level::Debug | Level::Info => style(record.args()),
        Level::Warn | Level::Error => style(record.args()).with(color).attribute(Attribute::Bold),
    };

    write!(w, "{:<5} [{}] {}\r", level, target, args)
}
