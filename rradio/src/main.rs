#![warn(clippy::pedantic)]
#![allow(clippy::used_underscore_binding)]

use anyhow::{Context, Result};
use tokio::sync::mpsc;

mod atomic_string;
mod config;
mod keyboard_commands;
mod log_error;
mod pipeline;
mod ports;
mod station;
mod tag;

fn main() -> Result<()> {
    let mut logger = flexi_logger::Logger::with_str("error")
        .format(log_format)
        .start()?;

    let mut config_path = String::from(option_env!("RRADIO_CONFIG_PATH").unwrap_or("config.toml"));

    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-c" | "--config" => {
                config_path = args.next().context("No config specified")?;
            }
            "-V" | "--version" => {
                println!("{} v{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            _ => return Err(anyhow::Error::msg(format!("Unhandled argument {:?}", arg))),
        }
    }

    let config = config::Config::load(config_path);

    logger.parse_new_spec(&config.log_level);

    let (commands_tx, commands_rx) = mpsc::unbounded_channel();

    let keyboard_commands_task = keyboard_commands::run(commands_tx, config.input_timeout);
    let (pipeline_task, player_state_rx, log_message_source) = pipeline::run(config, commands_rx)?;
    let tcp_task = ports::tcp_text::run(player_state_rx, log_message_source);

    let mut runtime = tokio::runtime::Runtime::new()?;
    runtime.spawn(pipeline_task);
    runtime.spawn(log_error::log_error(tcp_task));

    runtime.block_on(log_error::log_error(keyboard_commands_task));

    drop(runtime);

    logger.shutdown();

    Ok(())
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
