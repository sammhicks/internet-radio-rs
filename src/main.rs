#![warn(clippy::pedantic)]

use std::io::stdout;

use actix::prelude::*;
use anyhow::Result;

mod channel;
mod config;
mod keyboard_commands;
mod logger;
mod message;
mod pipeline;
mod playlist;
mod tag;

use logger::Logger;

fn main() -> Result<()> {
    log::set_boxed_logger(Logger::new(stdout()))?;
    log::set_max_level(log::LevelFilter::Trace);

    let config = config::load();

    log::set_max_level(config.log_level);

    gstreamer::init()?;

    let system = actix::System::new("internet-radio");

    let pipeline = pipeline::Controller::new(config.clone())?.start();

    keyboard_commands::KeyboardCommands::new(pipeline.recipient(), config.input_timeout)?.start();

    actix::Arbiter::spawn(async {
        if let Err(err) = tokio::signal::ctrl_c().await {
            log::error!("{:?}", err);
        } else {
            log::info!("interrupt received");
        }
        System::current().stop();
    });

    system.run().map_err(anyhow::Error::new)
}
