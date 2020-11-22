//! A radio station in rradio

use std::path::Path;

use rradio_messages::StationType;
pub use rradio_messages::{StationError, Track};

mod parse_custom;
mod parse_m3u;
mod parse_pls;

#[cfg(not(feature = "cd"))]
mod cd {
    pub fn tracks(
        _device: &str,
    ) -> Result<Vec<super::Track>, rradio_messages::CdError<crate::atomic_string::AtomicString>>
    {
        Err(rradio_messages::CdError::CdNotEnabled)
    }
}

#[cfg(all(feature = "cd", not(unix)))]
compile_error!("CD only supported on unix");

#[cfg(all(feature = "cd", unix))]
mod cd_unix;
#[cfg(all(feature = "cd", unix))]
use cd_unix as cd;

type Result<T> =
    std::result::Result<T, rradio_messages::StationError<crate::atomic_string::AtomicString>>;

#[derive(Clone, Debug, PartialEq)]
pub struct Credentials {
    username: String,
    password: String,
}

pub struct Playlist {
    pub station_index: Option<String>,
    pub station_title: Option<String>,
    pub station_type: rradio_messages::StationType,
    pub pause_before_playing: Option<std::time::Duration>,
    pub show_buffer: Option<bool>,
    pub tracks: Vec<Track>,
}

/// A station in rradio
#[derive(Debug, PartialEq)]
pub enum Station {
    UrlList {
        index: String,
        title: Option<String>,
        pause_before_playing: Option<std::time::Duration>,
        show_buffer: Option<bool>, // Show the user how full the gstreamer buffer is
        tracks: Vec<Track>,
    },
    FileServer {
        index: String,
        title: Option<String>,
        credentials: Credentials,
        show_buffer: Option<bool>, // Show the user how full the gstreamer buffer is
        remote_address: String,
    },
    CD {
        index: String,
        device: String,
    },
    Singleton {
        track: Track,
    },
}

fn stations_directory_io_error<T>(result: std::io::Result<T>) -> Result<T> {
    result.map_err(|err| {
        rradio_messages::StationError::StationsDirectoryIoError(err.to_string().into())
    })
}

fn playlist_error<T>(result: anyhow::Result<T>) -> Result<T> {
    result.map_err(|err| rradio_messages::StationError::BadStationFile(format!("{:#}", err).into()))
}

impl Station {
    /// Load the station with the given index from the given directory, if the index exists
    pub fn load(directory: impl AsRef<Path>, index: String) -> Result<Self> {
        for entry in stations_directory_io_error(std::fs::read_dir(directory.as_ref()))? {
            let entry = stations_directory_io_error(entry)?;
            let name = entry.file_name();

            if name.to_string_lossy().starts_with(&index) {
                let path = entry.path();
                return match entry
                    .path()
                    .extension()
                    .ok_or_else(|| StationError::BadStationFile("File has no extension".into()))?
                    .to_string_lossy()
                    .as_ref()
                {
                    "m3u" => playlist_error(parse_m3u::parse(path, index)),
                    "pls" => playlist_error(parse_pls::parse(path, index)),
                    "txt" => playlist_error(parse_custom::parse(path, index)),
                    extension => Err(StationError::BadStationFile(
                        format!("Unsupported format: \"{}\"", extension).into(),
                    )),
                };
            }
        }

        Err(rradio_messages::StationError::StationNotFound {
            index: index.into(),
            directory: directory.as_ref().display().to_string().into(),
        })
    }

    /// Create a station consisting of a single url.
    pub fn singleton(url: String) -> Self {
        Self::Singleton {
            track: Track {
                title: None,
                url,
                is_notification: false,
            },
        }
    }

    pub fn into_playlist(self) -> Result<Playlist> {
        match self {
            Station::UrlList {
                index,
                title,
                pause_before_playing,
                show_buffer,
                tracks,
            } => Ok(Playlist {
                station_index: Some(index),
                station_title: title,
                station_type: StationType::UrlList,
                pause_before_playing,
                show_buffer,
                tracks,
            }),
            Station::FileServer { .. } => Err(StationError::FileServerNotEnabled),
            Station::CD { index, device } => Ok(Playlist {
                station_index: Some(index),
                station_title: None,
                station_type: StationType::CD,
                pause_before_playing: None,
                show_buffer: None,
                tracks: cd::tracks(&device)?,
            }),
            Station::Singleton { track } => Ok(Playlist {
                station_index: None,
                station_title: None,
                station_type: StationType::UrlList,
                pause_before_playing: None,
                show_buffer: None,
                tracks: vec![track],
            }),
        }
    }
}
