//! A description of the rradio configuration file

use anyhow::{Context, Result};
use log::LevelFilter;
use tokio::time::Duration;

/// Notifications allow rradio to play sounds to notify the user of events
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct Notifications {
    pub ready: Option<String>,
    pub playlist_prefix: Option<String>,
    pub playlist_suffix: Option<String>,
    pub error: Option<String>,
}

/// A description of the rradio configuration file
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config {
    /// Where to find stations
    #[serde(default = "default_stations_directory")]
    pub stations_directory: String,

    /// The timeout when entering two digit station indices
    #[serde(default = "default_input_timeout", with = "humantime_serde")]
    pub input_timeout: Duration,

    /// The change in volume when the user increments or decrements the volume
    #[serde(default = "default_volume_offset")]
    pub volume_offset: i32,

    #[serde(default = "default_buffering_duration", with = "humantime_serde")]
    pub buffering_duration: Option<Duration>,

    /// Controls the logging level. See the [Log Specification](https://docs.rs/flexi_logger/latest/flexi_logger/struct.LogSpecification.html)
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// Notification sounds
    #[serde(default, rename = "Notifications")]
    pub notifications: Notifications,
}

impl Config {
    pub fn load(path: impl AsRef<std::path::Path>) -> Self {
        std::fs::read_to_string(path)
            .context("Failed to read config file")
            .and_then(|config| toml::from_str(&config).context("Failed to parse config file"))
            .map_err(|err| log::error!("{:#}", err))
            .unwrap_or_default()
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            stations_directory: default_stations_directory(),
            input_timeout: default_input_timeout(),
            volume_offset: default_volume_offset(),
            buffering_duration: default_buffering_duration(),
            log_level: default_log_level(),
            notifications: Notifications::default(),
        }
    }
}

fn default_stations_directory() -> String {
    String::from("stations")
}

const fn default_input_timeout() -> Duration {
    Duration::from_millis(2000)
}

const fn default_volume_offset() -> i32 {
    5
}

const fn default_buffering_duration() -> Option<Duration> {
    None
}

fn default_log_level() -> String {
    String::from("Warn")
}

struct LogLevelParser;

impl<'de> serde::de::Visitor<'de> for LogLevelParser {
    type Value = LevelFilter;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "A log level filter")
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
        match v {
            "Off" => Ok(LevelFilter::Off),
            "Error" => Ok(LevelFilter::Error),
            "Warn" => Ok(LevelFilter::Warn),
            "Info" => Ok(LevelFilter::Info),
            "Debug" => Ok(LevelFilter::Debug),
            "Trace" => Ok(LevelFilter::Trace),
            _ => Err(serde::de::Error::unknown_variant(
                v,
                &["Off", "Error", "Warn", "Info", "Debug", "Trace"],
            )),
        }
    }
}
