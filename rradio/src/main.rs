#![warn(clippy::pedantic)]
#![allow(clippy::struct_field_names)]

use anyhow::{Context, Result};
use tracing_subscriber::prelude::*;

mod config;
mod keyboard_commands;
mod pipeline;
mod ports;
mod station;
mod stream_select;
mod tag;
mod task;

fn main() -> Result<()> {
    let log_filter_reload_handle = setup_logging();

    let config_file_path = {
        let mut config_file_path_from_args = None;

        let mut args = std::env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "-c" | "--config" => {
                    if let Some(existing_config_file_path) = config_file_path_from_args
                        .replace(args.next().context("No config specified")?)
                    {
                        anyhow::bail!(
                            "configuration file already specified as {existing_config_file_path:?}"
                        );
                    }
                }
                "-V" | "--version" => {
                    println!("{} v{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
                    println!("rradio-messages v{}", rradio_messages::VERSION);
                    return Ok(());
                }
                _ => return Err(anyhow::Error::msg(format!("Unhandled argument {arg:?}"))),
            }
        }

        config_file_path_from_args.unwrap_or_else(|| {
            String::from(option_env!("RRADIO_CONFIG_PATH").unwrap_or("config.toml"))
        })
    };

    let config = config::Config::from_file(&config_file_path); // See config::Config::default() for default config

    log_filter_reload_handle
        .reload(config.log_level.clone().filter) // Filter logs as specified by config
        .context("Failed to reload logger filter")?;

    tracing::debug!(target: concat!(module_path!(), "::config"), "{config:?}");

    let (shutdown_handle, shutdown_signal) = task::ShutdownSignal::new();

    let (pipeline, port_channels_without_shutdown_signal) = pipeline::run(config.clone())?;

    let port_channels = port_channels_without_shutdown_signal.with_shutdown_signal(shutdown_signal);

    #[cfg(feature = "web")]
    let web_task = ports::web::run(
        port_channels.clone(),
        String::from(config.web_config.web_app_path.as_str()),
    );

    let keyboard_commands_task = keyboard_commands::run(port_channels.commands_tx.clone(), config);

    let tcp_binary_task = ports::tcp_binary::run(port_channels.clone());

    let tcp_text_task = ports::tcp_text::run(port_channels);

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?; // Setup the async runtime

    // Spawn pipeline task outside of shutdown signalling mechanism as it doesn't need to do a graceful shutdown
    runtime.spawn(pipeline);

    runtime.block_on(async {
        let wait_group = task::WaitGroup::new();

        // Start other tasks within shutdown signalling mechanism
        wait_group.spawn_task(tracing::error_span!("tcp_text"), tcp_text_task);
        wait_group.spawn_task(tracing::error_span!("tcp_binary"), tcp_binary_task);

        #[cfg(feature = "web")]
        wait_group.spawn_task(tracing::error_span!("web"), web_task);

        // Wait for the keyboard task to finish, i.e. when "Q" is pressed
        keyboard_commands_task.await;

        // Signal that tasks should shut down
        shutdown_handle.signal_shutdown();

        // Wait (with timeout) for tasks to shut down
        if tokio::time::timeout(std::time::Duration::from_secs(5), wait_group.wait())
            .await
            .is_err()
        {
            tracing::warn!("Not all tasks shutdown within time limit");
        }
    });

    Ok(())
}

fn setup_logging() -> tracing_subscriber::reload::Handle<
    tracing_subscriber::filter::Targets,
    tracing_subscriber::Registry,
> {
    let (log_filter, reload_handle) =
        tracing_subscriber::reload::Layer::new(config::LogLevelFilter::default().filter);

    tracing_subscriber::registry() // Register logging
        .with(log_filter) // Only output some of the logs
        .with(
            tracing_subscriber::fmt::Layer::default() // Write formatted logs ...
                .with_writer(std::sync::Mutex::new(ForceCR(std::io::stderr()))), // .. to stderr
        )
        .init();

    reload_handle
}

/// `ForceCR` is a wrapper around a [`std::io::Write`] which explicitly sends a "\r\n" as a newline, even if only a "\n" is written.
/// This is useful because `stdout` is in "Raw" Mode.
struct ForceCR<W: std::io::Write>(W);

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
