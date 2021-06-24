//! A description of the rradio configuration file

#[cfg(feature = "ping")]
use std::net::Ipv4Addr;

use log::LevelFilter;
use tokio::time::Duration;

use rradio_messages::{arcstr, ArcStr};

/// Notifications allow rradio to play sounds to notify the user of events
#[derive(Clone, Debug, Default, serde::Deserialize)]
#[serde(default)]
pub struct Notifications {
    pub ready: Option<ArcStr>,
    pub playlist_prefix: Option<ArcStr>,
    pub playlist_suffix: Option<ArcStr>,
    pub error: Option<ArcStr>,
}

/// A description of the rradio configuration file
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default)]
pub struct Config {
    /// Where to find stations
    pub stations_directory: ArcStr,

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
    pub log_level: ArcStr,

    /// Notification sounds
    #[serde(rename = "Notifications")]
    pub notifications: Notifications,

    #[cfg(feature = "ping")]
    pub ping_count: usize,

    #[cfg(feature = "ping")]
    pub gateway_address: Ipv4Addr,
}

impl Config {
    pub fn load(path: impl AsRef<std::path::Path> + Copy) -> Self {
        std::fs::read_to_string(path)
            .map_err(|err| {
                log::error!(
                    "Failed to read config file {:?}: {}",
                    path.as_ref().display(),
                    err
                )
            })
            .and_then(|config| {
                toml::from_str(&config).map_err(|err| {
                    log::error!(
                        "Failed to parse config file {:?}: {}",
                        path.as_ref().display(),
                        err
                    )
                })
            })
            .unwrap_or_default()
    }
}

#[cfg(all(feature = "ping", unix))]
fn default_gateway() -> Ipv4Addr {
    let path = "/proc/net/route";
    std::fs::read_to_string(path)
        .map_err(|err| log::error!("Failed to read {:?}: {}", path, err))
        .ok()
        .and_then(|route| {
            route.lines().find_map(|line| {
                let mut sections = line.split('\t').skip(1);

                let destination = sections.next()?;
                if destination != "00000000" {
                    return None;
                }

                let gateway = sections.next()?;

                Some(Ipv4Addr::from(
                    u32::from_str_radix(gateway, 16).ok()?.to_le_bytes(),
                ))
            })
        })
        .unwrap_or(Ipv4Addr::new(192, 168, 0, 1))
}

#[cfg(all(feature = "ping", not(unix)))]
fn default_gateway() -> Ipv4Addr {
    Ipv4Addr::LOCALHOST
}

impl Default for Config {
    fn default() -> Self {
        Self {
            stations_directory: arcstr::literal!("stations"),
            input_timeout: Duration::from_millis(2000),
            volume_offset: 5,
            buffering_duration: None,
            pause_before_playing_increment: Duration::from_secs(1),
            max_pause_before_playing: Duration::from_secs(5),
            log_level: arcstr::literal!("Info"),
            notifications: Notifications::default(),
            #[cfg(feature = "ping")]
            ping_count: 30,
            #[cfg(feature = "ping")]
            gateway_address: default_gateway(),
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
