//! A description of the rradio configuration file

use std::{collections::BTreeMap, fmt};

use tokio::time::Duration;

use rradio_messages::{arcstr, ArcStr};
use tracing_subscriber::filter::Targets;

#[derive(Clone)]
pub struct LogLevelFilter {
    pub filter: Targets,
}

impl<'de> serde::Deserialize<'de> for LogLevelFilter {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = toml::Value::deserialize(deserializer)?;

        Ok(Self {
            filter: if let Some(log_level_filter) = value.as_str() {
                log_level_filter
                    .parse()
                    .map_err(<D::Error as serde::de::Error>::custom)?
            } else {
                let log_level_filter = BTreeMap::<String, String>::deserialize(value)
                    .map_err(<D::Error as serde::de::Error>::custom)?;

                Targets::new().with_targets::<String, tracing::level_filters::LevelFilter>(
                    log_level_filter
                        .into_iter()
                        .map(|(target, level)| {
                            Ok((
                                target,
                                level.parse().map_err(|err| {
                                    <D::Error as serde::de::Error>::custom(format!(
                                        "{err}, got {level:?}"
                                    ))
                                })?,
                            ))
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                )
            },
        })
    }
}

impl fmt::Display for LogLevelFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.filter.fmt(f)
    }
}

impl fmt::Debug for LogLevelFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.to_string().fmt(f)
    }
}

impl Default for LogLevelFilter {
    fn default() -> Self {
        Self {
            filter: Targets::new().with_default(tracing::Level::WARN),
        }
    }
}

#[cfg(feature = "cd")]
pub mod cd {
    use rradio_messages::{arcstr, ArcStr};

    #[derive(Clone, Debug, serde::Deserialize)]
    #[serde(default)]
    pub struct Config {
        pub station: ArcStr,
        pub device: ArcStr,
    }

    impl Default for Config {
        fn default() -> Self {
            Self {
                station: arcstr::literal!("00"),
                device: arcstr::literal!("/dev/cdrom"),
            }
        }
    }
}

#[cfg(feature = "usb")]
pub mod usb {
    use std::path::PathBuf;

    use rradio_messages::{arcstr, ArcStr};

    #[derive(Clone, Debug, serde::Deserialize)]
    #[serde(default)]
    pub struct Config {
        pub station: ArcStr,
        pub device: ArcStr,
        pub path: PathBuf,
    }

    impl Default for Config {
        fn default() -> Self {
            Self {
                station: arcstr::literal!("01"),
                device: arcstr::literal!("/dev/sda1"),
                path: PathBuf::new(),
            }
        }
    }
}

#[cfg(feature = "ping")]
pub mod ping {
    use std::net::Ipv4Addr;

    use rradio_messages::{arcstr, ArcStr};

    #[derive(Clone, Debug, serde::Deserialize)]
    #[serde(default)]
    pub struct Config {
        pub remote_ping_count: usize,
        pub gateway_address: Ipv4Addr,
        pub initial_ping_address: ArcStr,
    }

    impl Default for Config {
        fn default() -> Self {
            Self {
                remote_ping_count: 30,
                gateway_address: default_gateway(),
                initial_ping_address: arcstr::literal!("8.8.8.8"),
            }
        }
    }

    fn default_gateway() -> Ipv4Addr {
        let path = "/proc/net/route";
        std::fs::read_to_string(path)
            .map_err(|err| tracing::error!("Failed to read {:?}: {}", path, err))
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
}

#[cfg(feature = "web")]
pub mod web {
    use rradio_messages::{arcstr, ArcStr};

    #[derive(Clone, Debug, serde::Deserialize)]
    #[serde(default)]
    pub struct Config {
        pub web_app_path: ArcStr,
    }

    impl Default for Config {
        fn default() -> Self {
            Self {
                web_app_path: arcstr::literal!("web_app"),
            }
        }
    }
}

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

    /// The volume on startup
    pub initial_volume: i32,

    /// The change in volume when the user increments or decrements the volume
    pub volume_offset: i32,

    #[serde(with = "humantime_serde")]
    pub buffering_duration: Option<Duration>,

    #[serde(with = "humantime_serde")]
    pub pause_before_playing_increment: Duration,

    #[serde(with = "humantime_serde")]
    pub max_pause_before_playing: Duration,

    #[serde(with = "humantime_serde")]
    pub smart_goto_previous_track_duration: Duration,

    pub maximum_error_recovery_attempts: usize,

    #[serde(with = "humantime_serde")]
    pub error_recovery_attempt_count_reset_time: Option<Duration>,

    pub log_level: LogLevelFilter,

    /// Notification sounds
    #[serde(rename = "Notifications")]
    pub notifications: Notifications,

    #[cfg(feature = "cd")]
    #[serde(rename = "CD")]
    pub cd_config: cd::Config,

    #[cfg(feature = "usb")]
    #[serde(rename = "USB")]
    pub usb_config: usb::Config,

    #[cfg(feature = "ping")]
    #[serde(rename = "ping")]
    pub ping_config: ping::Config,

    #[cfg(feature = "web")]
    #[serde(rename = "web")]
    pub web_config: web::Config,
}

impl Config {
    pub fn from_file(path: impl AsRef<std::path::Path> + Copy) -> Self {
        std::fs::read_to_string(path)
            .map_err(|err| {
                tracing::error!(
                    "Failed to read config file {:?}: {}",
                    path.as_ref().display(),
                    err
                );
            })
            .and_then(|config| {
                toml::from_str(&config).map_err(|err| {
                    tracing::error!(
                        "Failed to parse config file {:?}: {}",
                        path.as_ref().display(),
                        err
                    );
                })
            })
            .unwrap_or_default()
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            stations_directory: arcstr::literal!("stations"),
            input_timeout: Duration::from_millis(2000),
            initial_volume: 70,
            volume_offset: 5,
            buffering_duration: None,
            pause_before_playing_increment: Duration::from_secs(1),
            max_pause_before_playing: Duration::from_secs(5),
            smart_goto_previous_track_duration: Duration::from_secs(2),
            maximum_error_recovery_attempts: 5,
            error_recovery_attempt_count_reset_time: Some(Duration::from_secs(30)),
            log_level: LogLevelFilter::default(),
            notifications: Notifications::default(),
            #[cfg(feature = "cd")]
            cd_config: cd::Config::default(),
            #[cfg(feature = "usb")]
            usb_config: usb::Config::default(),
            #[cfg(feature = "ping")]
            ping_config: ping::Config::default(),
            #[cfg(feature = "web")]
            web_config: web::Config::default(),
        }
    }
}
