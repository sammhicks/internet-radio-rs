use std::fs::read_to_string;

use log::{error, LevelFilter};
use serde::{de, Deserializer};

use crate::channel::Channel;

#[derive(serde::Deserialize)]
pub struct Config {
    #[serde(default = "default_input_timeout_ms")]
    pub input_timeout_ms: u64,
    #[serde(deserialize_with = "parse_log_level", default = "default_log_level")]
    pub log_level: LevelFilter,
    #[serde(rename = "Station")]
    pub station: Vec<Channel>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            input_timeout_ms: default_input_timeout_ms(),
            log_level: default_log_level(),
            station: Vec::new(),
        }
    }
}

const fn default_input_timeout_ms() -> u64 {
    2000
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
    let config = match read_to_string("config.toml") {
        Ok(config) => config,
        Err(err) => {
            error!("Failed to read config file: {}", err);
            return Config::default();
        }
    };
    match toml::from_str(&config) {
        Ok(config) => config,
        Err(err) => {
            error!("Failed to parse config file: {}", err);
            Config::default()
        }
    }
}
