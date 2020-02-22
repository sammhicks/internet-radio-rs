use actix::clock::Duration;
use anyhow::{Context, Result};
use log::{error, LevelFilter};
use serde::{de, Deserializer};

#[derive(Clone, Debug, Default, serde::Deserialize)]
pub struct Notifications {
    pub success: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, serde::Deserialize)]
pub struct Config {
    #[serde(default = "default_channels_directory")]
    pub channels_directory: String,
    #[serde(
        rename = "input_timeout_ms",
        default = "default_input_timeout",
        deserialize_with = "deserialize_duration_millis"
    )]
    pub input_timeout: Duration,
    #[serde(default = "default_volume_offset_percent")]
    pub volume_offset_percent: i32,
    #[serde(deserialize_with = "parse_log_level", default = "default_log_level")]
    pub log_level: LevelFilter,
    #[serde(default, rename = "Notifications")]
    pub notifications: Notifications,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            channels_directory: default_channels_directory(),
            input_timeout: default_input_timeout(),
            volume_offset_percent: default_volume_offset_percent(),
            log_level: default_log_level(),
            notifications: Notifications::default(),
        }
    }
}

fn default_channels_directory() -> String {
    String::from("channels")
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

const fn default_volume_offset_percent() -> i32 {
    10
}

const fn default_log_level() -> LevelFilter {
    LevelFilter::Error
}

fn parse_log_level<'de, D: Deserializer<'de>>(deserializer: D) -> Result<LevelFilter, D::Error> {
    deserializer.deserialize_str(LogLevelParser)
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

pub fn load() -> Config {
    std::fs::read_to_string("config.toml")
        .context("Failed to read config file")
        .and_then(|config| toml::from_str(&config).context("Failed to parse config file"))
        .map_err(|err| error!("{:?}", err))
        .unwrap_or_default()
}
