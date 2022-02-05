//! A radio station in rradio
use std::{any::Any, fmt, sync::Arc};

use rradio_messages::{ArcStr, StationType};
pub use rradio_messages::{StationError as Error, Track};

mod parse_m3u;
mod parse_pls;
mod parse_upnp;

#[cfg(not(feature = "cd"))]
mod cd {
    pub fn tracks(_device: &str) -> Result<Vec<super::Track>, rradio_messages::CdError> {
        Err(rradio_messages::CdError::CdNotEnabled)
    }
}

#[cfg(all(feature = "cd", not(unix)))]
compile_error!("CD only supported on unix");

#[cfg(all(feature = "cd", unix))]
mod cd_unix;
#[cfg(all(feature = "cd", unix))]
use cd_unix as cd;

#[cfg(all(feature = "cd", unix))]
pub use cd::eject as eject_cd;

#[cfg(all(feature = "mount", not(unix)))]
compile_error!("Mounting only supported on unix");

#[cfg(all(feature = "mount", unix))]
mod mount;

#[derive(Debug, PartialEq)]
pub struct Credentials {
    username: String,
    password: String,
}

#[derive(Clone)]
pub struct PlaylistMetadata(Arc<dyn Any + Send + Sync>);

impl PlaylistMetadata {
    fn new(metadata: impl Any + Send + Sync + 'static) -> Self {
        Self(Arc::new(metadata))
    }
}

impl fmt::Debug for PlaylistMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PlaylistMetadata")
            .field(&(&*self.0).type_id())
            .finish()
    }
}

impl Default for PlaylistMetadata {
    fn default() -> Self {
        Self(Arc::new(()))
    }
}

pub struct PlaylistHandle(Box<dyn Any + Send + Sync>);

impl PlaylistHandle {
    fn new(handle: impl Any + Send + Sync + 'static) -> Self {
        Self(Box::new(handle))
    }
}

impl fmt::Debug for PlaylistHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PlaylistHandle")
            .field(&(&*self.0).type_id())
            .field(&self.0)
            .finish()
    }
}

impl Default for PlaylistHandle {
    fn default() -> Self {
        Self(Box::new(()))
    }
}

pub struct Playlist {
    pub station_index: Option<String>,
    pub station_title: Option<String>,
    pub station_type: rradio_messages::StationType,
    pub tracks: Vec<Track>,
    pub metadata: PlaylistMetadata,
    pub handle: PlaylistHandle,
}

/// A station in rradio
#[derive(Debug, PartialEq)]
pub enum Station {
    UrlList {
        index: String,
        title: Option<String>,
        tracks: Vec<Track>,
    },
    CD {
        index: String,
        device: String,
    },
    Usb {
        index: String,
        device: String,
        path: std::path::PathBuf,
    },
    Singleton {
        track: Track,
    },
}

fn stations_directory_io_error<T>(
    directory: &ArcStr,
    result: std::io::Result<T>,
) -> Result<T, Error> {
    result.map_err(|err| Error::StationsDirectoryIoError {
        directory: directory.clone(),
        err: err.to_string().into(),
    })
}

fn playlist_error<T>(result: anyhow::Result<T>) -> Result<T, Error> {
    result.map_err(|err| rradio_messages::StationError::BadStationFile(format!("{:#}", err).into()))
}

impl Station {
    /// Load the station with the given index from the given directory, if the index exists
    pub async fn load(config: &crate::config::Config, index: String) -> Result<Self, Error> {
        let directory = &config.stations_directory;

        #[cfg(feature = "cd")]
        if index.as_str() == config.cd_config.station {
            return Ok(Self::CD {
                index,
                device: config.cd_config.device.to_string(),
            });
        }

        #[cfg(feature = "usb")]
        if index.as_str() == config.usb_config.station {
            return Ok(Self::Usb {
                index,
                device: config.usb_config.device.to_string(),
                path: config.usb_config.path.clone(),
            });
        }

        for entry in stations_directory_io_error(directory, std::fs::read_dir(directory.as_str()))?
        {
            let entry = stations_directory_io_error(directory, entry)?;
            let name = entry.file_name();

            if name.to_string_lossy().starts_with(&index) {
                let path = entry.path();
                return match entry
                    .path()
                    .extension()
                    .ok_or_else(|| Error::BadStationFile("File has no extension".into()))?
                    .to_string_lossy()
                    .as_ref()
                {
                    "m3u" => playlist_error(parse_m3u::parse(&path, index)),
                    "pls" => playlist_error(parse_pls::parse(path, index)),
                    "upnp" => playlist_error(parse_upnp::parse(&path, index).await),
                    extension => Err(Error::BadStationFile(
                        format!("Unsupported format: \"{}\"", extension).into(),
                    )),
                };
            }
        }

        Err(rradio_messages::StationError::StationNotFound {
            index: index.into(),
            directory: directory.clone(),
        })
    }

    /// Create a station consisting of a single url.
    pub fn singleton(url: ArcStr) -> Self {
        Self::Singleton {
            track: Track {
                title: None,
                album: None,
                artist: None,
                url,
                is_notification: false,
            },
        }
    }

    pub fn index(&self) -> Option<&str> {
        match self {
            Station::UrlList { index, .. }
            | Station::CD { index, .. }
            | Station::Usb { index, .. } => Some(index.as_str()),
            Station::Singleton { .. } => None,
        }
    }

    pub fn into_playlist(self, metadata: Option<&PlaylistMetadata>) -> Result<Playlist, Error> {
        match self {
            Station::UrlList {
                index,
                title,
                tracks,
            } => Ok(Playlist {
                station_index: Some(index),
                station_title: title,
                station_type: StationType::UrlList,
                tracks,
                metadata: PlaylistMetadata::default(),
                handle: PlaylistHandle::default(),
            }),
            Station::CD { index, device } => Ok(Playlist {
                station_index: Some(index),
                station_title: None,
                station_type: StationType::CD,
                tracks: cd::tracks(&device)?,
                metadata: PlaylistMetadata::default(),
                handle: PlaylistHandle::default(),
            }),
            #[cfg(not(all(feature = "usb", unix)))]
            Station::Usb { .. } => Err(rradio_messages::MountError::UsbNotEnabled.into()),
            #[cfg(all(feature = "usb", unix))]
            Station::Usb {
                index,
                device,
                path,
            } => {
                let (tracks, metadata, handle) = mount::usb(&device, &path, metadata)?;
                Ok(Playlist {
                    station_index: Some(index),
                    station_title: None,
                    station_type: StationType::Usb,
                    tracks,
                    metadata,
                    handle,
                })
            }
            Station::Singleton { track } => Ok(Playlist {
                station_index: None,
                station_title: None,
                station_type: StationType::UrlList,
                tracks: vec![track],
                metadata: PlaylistMetadata::default(),
                handle: PlaylistHandle::default(),
            }),
        }
    }
}
