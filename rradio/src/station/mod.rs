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

    pub fn index(&self) -> Option<&str> {
        match &self {
            Station::UrlList { index, .. }
            | Station::FileServer { index, .. }
            | Station::CD { index, .. } => Some(index.as_str()),
            Station::Singleton { .. } => None,
        }
    }

    pub fn title(&self) -> Option<&str> {
        match &self {
            Station::UrlList { title, .. } | Station::FileServer { title, .. } => {
                title.as_ref().map(String::as_str)
            }
            Station::CD { .. } => Some("CD"),
            Station::Singleton { .. } => None,
        }
    }

    pub fn tracks(&self) -> Result<Vec<Track>> {
        match self {
            Self::UrlList { tracks, .. } => Ok(tracks.clone()),
            Self::FileServer { .. } => anyhow::bail!("FileServer not supported yet"),
            Self::CD { device, .. } => cd::tracks(device.as_str()),
            Self::Singleton { track } => Ok(vec![track.clone()]),
        }
    }

    pub fn pause_before_playing(&self) -> Option<std::time::Duration> {
        if let Self::UrlList {
            pause_before_playing,
            ..
        } = self
        {
            *pause_before_playing
        } else {
            None
        }
    }
}
