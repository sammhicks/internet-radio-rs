#![warn(clippy::pedantic)]
#![allow(clippy::used_underscore_binding)]

use anyhow::{Context, Result};

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

    let (shutdown_handle, shutdown_signal) = ports::ShutdownSignal::new();

    let (pipeline_task, port_channels) = pipeline::run(config.clone())?;

    let port_channels = port_channels.with_shutdown_signal(shutdown_signal);

    let keyboard_commands_task = keyboard_commands::run(port_channels.commands.clone(), config);

    #[cfg(feature = "web")]
    let web_task = ports::web::run(port_channels.clone());

    let tcp_msgpack_task = ports::tcp_msgpack::run(port_channels.clone());
    let tcp_text_task = ports::tcp_text::run(port_channels);

    let mut runtime = tokio::runtime::Runtime::new()?;

    // These tasks don't need special shutdown
    runtime.spawn(pipeline_task);

    runtime.block_on(async move {
        let wait_group = ports::WaitGroup::new();

        // These tasks have a special shutdown procedure
        wait_group.spawn_task(tcp_text_task);
        wait_group.spawn_task(tcp_msgpack_task);
        #[cfg(feature = "web")]
        wait_group.spawn_task(web_task);

        log_error::log_error(keyboard_commands_task).await;

        drop(shutdown_handle);

        if tokio::time::timeout(std::time::Duration::from_secs(5), wait_group.wait())
            .await
            .is_err()
        {
            log::error!("Not all tasks shutdown within time limit");
        }
    });

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
