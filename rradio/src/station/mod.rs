//! A radio station in rradio
use std::{any::Any, fmt, sync::Arc};

use rradio_messages::{arcstr, StationIndex, StationType};
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

impl TypeName for () {
    const TYPE_NAME: &'static str = "()";
}

#[derive(Clone)]
pub struct Metadata(Arc<dyn Any + Send + Sync>);

impl Metadata {
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

impl fmt::Debug for Metadata {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PlaylistMetadata")
            .field(&(*self.0).type_id())
            .finish()
    }
}

impl Default for Metadata {
    fn default() -> Self {
        Self(Arc::new(()))
    }
}

pub struct Handle(Box<dyn Any + Send + Sync>);

impl fmt::Debug for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PlaylistHandle")
            .field(&(*self.0).type_id())
            .field(&self.0)
            .finish()
    }
}

impl Default for Handle {
    fn default() -> Self {
        Self(Box::new(()))
    }
}

#[derive(Debug)]
pub struct Station {
    pub index: Option<StationIndex>,
    pub title: Option<String>,
    pub r#type: rradio_messages::StationType,
    pub tracks: Vec<Track>,
    pub metadata: Metadata,
    pub handle: Handle,
}

// impl Station {
//     pub fn from_url_list()
// }

struct BadStationFile {
    error: anyhow::Error,
}

impl<E: std::error::Error + Send + Sync + 'static> From<E> for BadStationFile {
    fn from(error: E) -> Self {
        Self {
            error: error.into(),
        }
    }
}

impl From<BadStationFile> for Error {
    fn from(BadStationFile { error }: BadStationFile) -> Self {
        rradio_messages::StationError::BadStationFile(format!("{error:#}").into())
    }
}

struct PartialInfo<'a> {
    pub title: Option<&'a str>,
}

pub struct Info<'a> {
    pub title: Option<&'a str>,
    pub source_type: StationType,
}

struct StationTitle {
    station_title: String,
}

trait StationLoader: Sized {
    type Metadata: Any + TypeName + Clone + Send + Sync + 'static;
    type Handle: Send + Sync + 'static;
    type Track: Into<rradio_messages::Track>;
    type Error: Into<Error>;

    const STATION_TYPE: StationType;

    async fn load_station_parts(
        self,
        metadata: Option<Self::Metadata>,
        publish_station_info: impl FnOnce(PartialInfo),
    ) -> Result<
        (
            Option<StationTitle>,
            Vec<Self::Track>,
            Self::Metadata,
            Self::Handle,
        ),
        Self::Error,
    >;

    async fn load_station(
        self,
        index: StationIndex,
        metadata: Option<&Metadata>,
        publish_station_info: impl FnOnce(Info),
    ) -> Result<Station, Error> {
        self.load_station_parts(
            metadata.as_ref().and_then(|metadata| metadata.get()),
            |PartialInfo { title }| {
                publish_station_info(Info {
                    title,
                    source_type: Self::STATION_TYPE,
                });
            },
        )
        .await
        .map(|(station_title, tracks, metadata, handle)| Station {
            index: Some(index),
            title: station_title.map(|StationTitle { station_title }| station_title),
            r#type: Self::STATION_TYPE,
            tracks: tracks.into_iter().map(Into::into).collect(),
            metadata: Metadata::new(metadata),
            handle: Handle(Box::new(handle)),
        })
        .map_err(Into::into)
    }
}

pub async fn load_station_with_index(
    config: &crate::config::Config,
    index: StationIndex,
    metadata: Option<&Metadata>,
    publish_station_info: impl FnOnce(Info),
) -> Result<Station, Error> {
    #[cfg(feature = "cd")]
    if index.as_str() == config.cd_config.station {
        return cd::Loader {
            device: &config.cd_config.device,
        }
        .load_station(index, metadata, publish_station_info)
        .await;
    }

    let directory = &config.stations_directory;

    for entry in
        std::fs::read_dir(directory.as_str()).map_err(|err| Error::StationsDirectoryIoError {
            directory: directory.clone(),
            err: arcstr::format!("{err}"),
        })?
    {
        let entry = entry.map_err(|err| Error::StationsDirectoryIoError {
            directory: directory.clone(),
            err: arcstr::format!("{err}"),
        })?;

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
                "m3u" => {
                    parse_m3u::Loader { path }
                        .load_station(index, metadata, publish_station_info)
                        .await
                }
                "pls" => {
                    parse_pls::Loader { path }
                        .load_station(index, metadata, publish_station_info)
                        .await
                }
                "upnp" => {
                    parse_upnp::Loader { path }
                        .load_station(index, metadata, publish_station_info)
                        .await
                }
                #[cfg(feature = "smb")]
                "smb" => {
                    smb::Loader { path }
                        .load_station(index, metadata, publish_station_info)
                        .await
                }
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
