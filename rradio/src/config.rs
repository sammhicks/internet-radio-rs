//! A description of the rradio configuration file

use log::LevelFilter;
use tokio::time::Duration;

/// Notifications allow rradio to play sounds to notify the user of events
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct Notifications {
    pub ready: Option<String>,
    pub playlist_prefix: Option<String>,
    pub playlist_suffix: Option<String>,
    pub error: Option<String>,
}

/// A description of the rradio configuration file
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    /// Where to find stations
    pub stations_directory: String,

    /// The timeout when entering two digit station indices
    #[serde(with = "humantime_serde")]
    pub input_timeout: Duration,

    /// The change in volume when the user increments or decrements the volume
    pub volume_offset: i32,

    #[serde(with = "humantime_serde")]
    pub buffering_duration: Option<Duration>,

    #[serde(with = "humantime_serde")]
    pub pause_before_playing_increment: Duration,

    #[serde(with = "humantime_serde")]
    pub max_pause_before_playing: Duration,

    /// Controls the logging level. See the [Log Specification](https://docs.rs/flexi_logger/latest/flexi_logger/struct.LogSpecification.html)
    pub log_level: String,

    /// Notification sounds
    #[serde(rename = "Notifications")]
    pub notifications: Notifications,
}

impl Config {
    pub fn load(path: impl AsRef<std::path::Path>) -> Self {
        std::fs::read_to_string(path)
            .map_err(|err| log::error!("Failed to read config file: {:?}", err))
            .and_then(|config| {
                toml::from_str(&config)
                    .map_err(|err| log::error!("Failed to parse config file: {:?}", err))
            })
            .unwrap_or_default()
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            stations_directory: String::from("stations"),
            input_timeout: Duration::from_millis(2000),
            volume_offset: 5,
            buffering_duration: None,
            pause_before_playing_increment: Duration::from_secs(1),
            max_pause_before_playing: Duration::from_secs(5),
            log_level: String::from("Info"),
            notifications: Notifications::default(),
        }
    }
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
