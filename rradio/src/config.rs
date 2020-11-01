//! A description of the rradio configuration file

use anyhow::{Context, Result};
use log::LevelFilter;
use serde::{de, Deserializer};
use tokio::time::Duration;

/// Notifications allow rradio to play sounds to notify the user of events
#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct Notifications {
    pub success: Option<String>,
    pub error: Option<String>,
}

/// A description of the rradio configuration file
#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config {
    /// Where to find stations
    #[serde(default = "default_stations_directory")]
    pub stations_directory: String,

    /// The timeout when entering two digit station indices
    #[serde(
        rename = "input_timeout_ms",
        default = "default_input_timeout",
        deserialize_with = "deserialize_duration_millis"
    )]
    pub input_timeout: Duration,

    /// The change in volume when the user increments or decrements the volume
    #[serde(default = "default_volume_offset")]
    pub volume_offset: i32,

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

fn deserialize_duration_millis<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Duration, D::Error> {
    deserializer.deserialize_u64(DurationMillisParser)
}

struct DurationMillisParser;

impl<'de> de::Visitor<'de> for DurationMillisParser {
    type Value = Duration;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "a Duration in milliseconds")
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
        use std::convert::TryFrom;
        u64::try_from(v)
            .map(Duration::from_millis)
            .map_err(|_| de::Error::invalid_value(de::Unexpected::Signed(v), &self))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
        Ok(Duration::from_millis(v))
    }
}

const fn default_volume_offset() -> i32 {
    5
}

fn default_log_level() -> String {
    String::from("Warn")
}

struct LogLevelParser;

impl<'de> de::Visitor<'de> for LogLevelParser {
    type Value = LevelFilter;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(formatter, "A log level filter")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
        match v {
            "Off" => Ok(LevelFilter::Off),
            "Error" => Ok(LevelFilter::Error),
            "Warn" => Ok(LevelFilter::Warn),
            "Info" => Ok(LevelFilter::Info),
            "Debug" => Ok(LevelFilter::Debug),
            "Trace" => Ok(LevelFilter::Trace),
            _ => Err(de::Error::unknown_variant(
                v,
                &["Off", "Error", "Warn", "Info", "Debug", "Trace"],
            )),
        }
    }
}
