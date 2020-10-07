//! A radio station in rradio

use std::path::Path;

use anyhow::{Context, Error, Result};

pub use rradio_messages::Track;

mod parse_custom;
mod parse_m3u;
mod parse_pls;

#[cfg(not(feature = "cd"))]
mod cd {
    pub fn tracks(_device: &str) -> anyhow::Result<Vec<super::Track>> {
        anyhow::bail!("CD support not enabled");
    }
}

#[cfg(all(feature = "cd", not(unix)))]
compile_error!("CD only supported on unix");

#[cfg(all(feature = "cd", unix))]
mod cd_unix;
#[cfg(all(feature = "cd", unix))]
use cd_unix as cd;

#[derive(Clone, Debug, PartialEq)]
pub struct Credentials {
    username: String,
    password: String,
}

pub struct Playlist {
    pub station_index: Option<String>,
    pub station_title: Option<String>,
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

impl Station {
    /// Load the station with the given index from the given directory, if the index exists
    pub fn load(directory: impl AsRef<Path>, index: String) -> Result<Self> {
        for entry in std::fs::read_dir(&directory)? {
            let entry = entry?;
            let name = entry.file_name();

            if name.to_string_lossy().starts_with(&index) {
                let path = entry.path();
                return match entry
                    .path()
                    .extension()
                    .context("File has no extension")?
                    .to_string_lossy()
                    .as_ref()
                {
                    "m3u" => parse_m3u::parse(path, index),
                    "pls" => parse_pls::parse(path, index),
                    "txt" => parse_custom::parse(path, index),
                    extension => Err(Error::msg(format!("Unsupported format: \"{}\"", extension))),
                };
            }
        }

        Err(anyhow::Error::msg(format!(
            "No station {} specified in \"{}\"",
            index,
            directory.as_ref().display()
        )))
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
                pause_before_playing,
                show_buffer,
                tracks,
            }),
            Station::FileServer { .. } => anyhow::bail!("FileServer not supported yet"),
            Station::CD { index, device } => Ok(Playlist {
                station_index: Some(index),
                station_title: None,
                pause_before_playing: None,
                show_buffer: None,
                tracks: cd::tracks(&device)?,
            }),
            Station::Singleton { track } => Ok(Playlist {
                station_index: None,
                station_title: None,
                pause_before_playing: None,
                show_buffer: None,
                tracks: vec![track],
            }),
        }
    }
}
