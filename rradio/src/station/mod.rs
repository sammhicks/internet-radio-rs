//! A radio station in rradio

use std::path::Path;

use rradio_messages::{ArcStr, StationType};
pub use rradio_messages::{StationError as Error, Track};

mod parse_custom;
mod parse_m3u;
mod parse_pls;

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

enum InnerHandle {
    None,
    #[cfg(all(feature = "mount", unix))]
    Mount(mount::Handle),
}

impl Default for InnerHandle {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Default)]
pub struct Handle(InnerHandle);

pub struct Playlist {
    pub station_index: Option<String>,
    pub station_title: Option<String>,
    pub station_type: rradio_messages::StationType,
    pub pause_before_playing: Option<std::time::Duration>,
    pub show_buffer: Option<bool>,
    pub tracks: Vec<Track>,
    pub handle: Handle,
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
        shuffle: bool,
    },
    SambaServer {
        index: String,
        title: Option<String>,
        credentials: Credentials,
        show_buffer: Option<bool>, // Show the user how full the gstreamer buffer is
        remote_address: String,
        shuffle: bool,
    },
    CD {
        index: String,
        device: String,
    },
    Usb {
        index: String,
        device: String,
        shuffle: bool,
    },
    Singleton {
        track: Track,
    },
}

fn stations_directory_io_error<T>(
    directory_name: impl AsRef<Path>,
    result: std::io::Result<T>,
) -> Result<T, Error> {
    result.map_err(|err| Error::StationsDirectoryIoError {
        directory: directory_name.as_ref().display().to_string().into(),
        err: err.to_string().into(),
    })
}

fn playlist_error<T>(result: anyhow::Result<T>) -> Result<T, Error> {
    result.map_err(|err| rradio_messages::StationError::BadStationFile(format!("{:#}", err).into()))
}

fn shuffle_tracks(mut tracks: Vec<Track>, shuffle: bool) -> Vec<Track> {
    use rand::seq::SliceRandom;

    if shuffle {
        tracks.shuffle(&mut rand::thread_rng());
    }

    tracks
}

impl Station {
    /// Load the station with the given index from the given directory, if the index exists
    pub fn load(directory: impl AsRef<Path> + Copy, index: String) -> Result<Self, Error> {
        for entry in stations_directory_io_error(directory, std::fs::read_dir(directory.as_ref()))?
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
                    "txt" => playlist_error(parse_custom::parse(path, index)),
                    extension => Err(Error::BadStationFile(
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

    pub fn into_playlist(self) -> Result<Playlist, Error> {
        match self {
            Station::UrlList {
                index,
                title,
                pause_before_playing,
                show_buffer,
                tracks,
                shuffle,
            } => Ok(Playlist {
                station_index: Some(index),
                station_title: title,
                station_type: StationType::UrlList,
                pause_before_playing,
                show_buffer,
                tracks: shuffle_tracks(tracks, shuffle),
                handle: Handle::default(),
            }),
            #[cfg(not(all(feature = "samba", unix)))]
            Station::SambaServer { .. } => Err(rradio_messages::MountError::SambaNotEnabled.into()),
            #[cfg(all(feature = "samba", unix))]
            Station::SambaServer {
                index,
                title,
                credentials,
                show_buffer,
                remote_address,
                shuffle,
            } => {
                let (handle, tracks) = mount::samba(&remote_address, &credentials)?;
                Ok(Playlist {
                    station_index: Some(index),
                    station_title: title,
                    station_type: StationType::Samba,
                    pause_before_playing: None,
                    show_buffer,
                    tracks: shuffle_tracks(tracks, shuffle),
                    handle: Handle(InnerHandle::Mount(handle)),
                })
            }
            Station::CD { index, device } => Ok(Playlist {
                station_index: Some(index),
                station_title: None,
                station_type: StationType::CD,
                pause_before_playing: None,
                show_buffer: None,
                tracks: cd::tracks(&device)?,
                handle: Handle::default(),
            }),
            #[cfg(not(all(feature = "usb", unix)))]
            Station::Usb { .. } => Err(rradio_messages::MountError::UsbNotEnabled.into()),
            #[cfg(all(feature = "usb", unix))]
            Station::Usb {
                index,
                device,
                shuffle,
            } => {
                let (handle, tracks) = mount::usb(&device)?;
                Ok(Playlist {
                    station_index: Some(index),
                    station_title: None,
                    station_type: StationType::Usb,
                    pause_before_playing: None,
                    show_buffer: None,
                    tracks: shuffle_tracks(tracks, shuffle),
                    handle: Handle(InnerHandle::Mount(handle)),
                })
            }
            Station::Singleton { track } => Ok(Playlist {
                station_index: None,
                station_title: None,
                station_type: StationType::UrlList,
                pause_before_playing: None,
                show_buffer: None,
                tracks: vec![track],
                handle: Handle::default(),
            }),
        }
    }
}
