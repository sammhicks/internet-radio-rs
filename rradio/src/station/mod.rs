//! A radio station in rradio
use std::{any::Any, fmt, sync::Arc};

use rradio_messages::{arcstr, ArcStr, StationIndex, StationType};
pub use rradio_messages::{StationError as Error, Track};

mod parse_m3u;
mod parse_pls;
mod parse_upnp;

#[cfg(feature = "mount")]
mod mount;

#[cfg(feature = "cd")]
mod cd;

#[cfg(feature = "cd")]
pub use cd::eject as eject_cd;

#[cfg(feature = "smb")]
mod smb;

#[cfg(feature = "usb")]
mod usb;

trait TypeName {
    const TYPE_NAME: &'static str;
}

#[derive(Clone)]
pub struct PlaylistMetadata(Arc<dyn Any + Send + Sync>);

impl PlaylistMetadata {
    fn new(metadata: impl Any + Send + Sync + 'static) -> Self {
        Self(Arc::new(metadata))
    }

    fn get<T: Any + TypeName + Clone>(&self) -> Option<T> {
        self.0
            .downcast_ref()
            .or_else(|| {
                tracing::error!(
                    "Metadata is not {} ({:?}), but is {:?}",
                    T::TYPE_NAME,
                    std::any::TypeId::of::<T>(),
                    &(*self.0).type_id()
                );

                None
            })
            .cloned()
    }
}

impl fmt::Debug for PlaylistMetadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PlaylistMetadata")
            .field(&(*self.0).type_id())
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
    #[cfg(feature = "mount")]
    fn new(handle: impl Any + Send + Sync + 'static) -> Self {
        Self(Box::new(handle))
    }
}

impl fmt::Debug for PlaylistHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PlaylistHandle")
            .field(&(*self.0).type_id())
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
    pub station_index: Option<StationIndex>,
    pub station_title: Option<String>,
    pub station_type: rradio_messages::StationType,
    pub tracks: Vec<Track>,
    pub metadata: PlaylistMetadata,
    pub handle: PlaylistHandle,
}

/// A station description
#[derive(Debug)]
pub enum Station {
    UrlList {
        index: Option<StationIndex>,
        title: Option<String>,
        tracks: Vec<Track>,
    },
    #[cfg(feature = "cd")]
    CD {
        index: StationIndex,
        device: String,
    },
    #[cfg(feature = "smb")]
    Smb(smb::Station),
    #[cfg(feature = "usb")]
    Usb {
        index: StationIndex,
        device: String,
        path: std::path::PathBuf,
    },
    UPnP(parse_upnp::Station),
}

/// Convert an [`std::io::Error`] into a [`rradio_messages::StationError::StationsDirectoryIoError`]
fn stations_directory_io_error<T>(
    directory: &ArcStr,
    result: std::io::Result<T>,
) -> Result<T, Error> {
    result.map_err(|err| Error::StationsDirectoryIoError {
        directory: directory.clone(),
        err: arcstr::format!("{err}"),
    })
}

/// Convert an [`anyhow::Error`] into a [`rradio_messages::StationError::BadStationFile`]
fn playlist_error<T>(result: anyhow::Result<T>) -> Result<T, Error> {
    result.map_err(|err| rradio_messages::StationError::BadStationFile(format!("{err:#}").into()))
}

impl Station {
    /// Load the station with the given index from the given directory, if the index exists
    pub fn load(config: &crate::config::Config, index: StationIndex) -> Result<Self, Error> {
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

            if name.to_string_lossy().starts_with(index.as_str()) {
                let path = entry.path();
                return match entry
                    .path()
                    .extension()
                    .ok_or_else(|| Error::BadStationFile("File has no extension".into()))?
                    .to_string_lossy()
                    .as_ref()
                {
                    "m3u" => playlist_error(parse_m3u::from_file(&path, index)),
                    "pls" => playlist_error(parse_pls::from_file(&path, index)),
                    "upnp" => playlist_error(parse_upnp::from_file(&path, index)),
                    #[cfg(feature = "smb")]
                    "smb" => playlist_error(smb::from_file(&path, index)),
                    extension => Err(Error::BadStationFile(
                        format!("Unsupported format: \"{extension}\"").into(),
                    )),
                };
            }
        }

        Err(rradio_messages::StationError::StationNotFound {
            index,
            directory: directory.clone(),
        })
    }

    pub fn index(&self) -> Option<&StationIndex> {
        match self {
            Station::UrlList { index, .. } => index.as_ref(),
            #[cfg(feature = "cd")]
            Station::CD { index, .. } => Some(index),
            #[cfg(feature = "smb")]
            Self::Smb(station) => Some(station.index()),
            #[cfg(feature = "usb")]
            Station::Usb { index, .. } => Some(index),
            Station::UPnP(station) => Some(station.index()),
        }
    }

    pub fn title(&self) -> Option<&str> {
        match self {
            Station::UrlList { title, .. } => title.as_deref(),
            #[cfg(feature = "cd")]
            Station::CD { .. } => None,
            #[cfg(feature = "smb")]
            Self::Smb(station) => Some(station.title()),
            #[cfg(feature = "usb")]
            Station::Usb { .. } => None,
            Station::UPnP(station) => station.title(),
        }
    }

    pub fn station_type(&self) -> StationType {
        match self {
            Station::UrlList { .. } => StationType::UrlList,
            #[cfg(feature = "cd")]
            Station::CD { .. } => StationType::CD,
            #[cfg(feature = "smb")]
            Self::Smb(..) => StationType::UPnP,
            #[cfg(feature = "usb")]
            Station::Usb { .. } => StationType::Usb,
            Station::UPnP(..) => StationType::UPnP,
        }
    }

    #[allow(clippy::unnecessary_wraps)]
    pub async fn into_playlist(
        self,
        metadata: Option<&PlaylistMetadata>,
    ) -> Result<Playlist, Error> {
        match self {
            Station::UrlList {
                index,
                title,
                tracks,
            } => Ok(Playlist {
                station_index: index,
                station_title: title,
                station_type: StationType::UrlList,
                tracks,
                metadata: PlaylistMetadata::default(),
                handle: PlaylistHandle::default(),
            }),
            #[cfg(feature = "cd")]
            Station::CD { index, device } => Ok(Playlist {
                station_index: Some(index),
                station_title: None,
                station_type: StationType::CD,
                tracks: cd::tracks(&device)?,
                metadata: PlaylistMetadata::default(),
                handle: PlaylistHandle::default(),
            }),
            #[cfg(feature = "smb")]
            Self::Smb(station) => station.into_playlist(metadata).map_err(Error::MountError),
            #[cfg(feature = "usb")]
            Station::Usb {
                index,
                device,
                path,
            } => {
                let (tracks, metadata, handle) = usb::load(&device, &path, metadata)?;
                Ok(Playlist {
                    station_index: Some(index),
                    station_title: None,
                    station_type: StationType::Usb,
                    tracks,
                    metadata,
                    handle,
                })
            }
            Station::UPnP(station) => station.into_playlist(metadata).await.map_err(|err| {
                rradio_messages::StationError::UPnPError(arcstr::format!("{err:#}"))
            }),
        }
    }
}
