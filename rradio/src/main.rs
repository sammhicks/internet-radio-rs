#![warn(clippy::pedantic)]
#![allow(clippy::used_underscore_binding)]

use anyhow::{Context, Result};
use tracing_subscriber::prelude::*;

mod config;
mod keyboard_commands;
mod pipeline;
mod ports;
mod station;
mod tag;
mod task;

fn main() -> Result<()> {
    let (filter, reload_handle) =
        tracing_subscriber::reload::Layer::new(None::<tracing_subscriber::EnvFilter>);

    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::Layer::default()
                .with_writer(std::sync::Mutex::new(ForceCR(std::io::stdout()))),
        )
        .init();

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

    let config = config::Config::load(&config_path);

    reload_handle
        .reload(Some(tracing_subscriber::EnvFilter::new(&config.log_level)))
        .context("Failed to reload logger filter")?;

    tracing::info!("Config: {:?}", config);

    let (shutdown_handle, shutdown_signal) = task::ShutdownSignal::new();

    let (pipeline_task, port_channels) = pipeline::run(config.clone())?;

    let port_channels = port_channels.with_shutdown_signal(shutdown_signal);

    #[cfg(feature = "web")]
    let web_task = ports::web::run(
        port_channels.clone(),
        config.web_config.web_app_path.as_str().to_owned(),
    );

    let keyboard_commands_task = keyboard_commands::run(port_channels.commands_tx.clone(), config);

    let tcp_msgpack_task = ports::tcp_msgpack::run(port_channels.clone());
    let tcp_text_task = ports::tcp_text::run(port_channels);

    let runtime = tokio::runtime::Runtime::new()?;

    // These tasks don't need special shutdown
    runtime.spawn(pipeline_task);

    runtime.block_on(async move {
        let wait_group = task::WaitGroup::new();

        // These tasks have a special shutdown procedure
        wait_group.spawn_task(tcp_text_task);
        wait_group.spawn_task(tcp_msgpack_task);
        #[cfg(feature = "web")]
        wait_group.spawn_task(web_task);

        keyboard_commands_task.await;

        drop(shutdown_handle);

        if tokio::time::timeout(std::time::Duration::from_secs(5), wait_group.wait())
            .await
            .is_err()
        {
            tracing::error!("Not all tasks shutdown within time limit");
        }
    });

    Ok(())
}

struct ForceCR<W>(W);

impl<W: std::io::Write> std::io::Write for ForceCR<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        for segment in buf.split(|&b| b == b'\n') {
            if !segment.is_empty() {
                self.0.write_all(segment)?;
                self.0.write_all(b"\r\n")?;
            }
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}
