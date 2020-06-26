#![warn(clippy::pedantic)]

use anyhow::Result;
use futures::FutureExt;
use std::io::stdout;
use tokio::sync::mpsc;

mod channel;
mod config;
mod keyboard_commands;
mod logger;
mod message;
mod pipeline;
mod playlist;
mod tag;

use logger::Logger;

#[tokio::main]
async fn main() -> Result<()> {
    log::set_boxed_logger(Logger::new(stdout()))?;
    log::set_max_level(log::LevelFilter::Trace);

    let config = config::load();

    log::set_max_level(config.log_level);

    gstreamer::init()?;

    let (commands_tx, commands_rx) = mpsc::unbounded_channel();

    let keyboard_commands_task = keyboard_commands::run(commands_tx.clone(), config.input_timeout);
    let pipeline_task = pipeline::run(config, commands_rx);

    futures::future::select_all(vec![keyboard_commands_task.boxed(), pipeline_task.boxed()])
        .await
        .0
}
