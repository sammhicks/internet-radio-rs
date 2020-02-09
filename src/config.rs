use std::fs::read_to_string;

use log::error;

use crate::channel::Channel;

#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct Config {
    #[serde(rename = "Input Timeout ms", default = "default_input_timeout_ms")]
    pub input_timeout_ms: u64,
    #[serde(rename = "Station")]
    pub station: Vec<Channel>,
}

fn default_input_timeout_ms() -> u64 {
    2000
}

pub fn load_config() -> Config {
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
            return Config::default();
        }
    }
}
