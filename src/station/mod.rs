//! A radio station in rradio

use std::path::Path;

use anyhow::{Context, Error, Result};

mod parse_m3u;
mod parse_pls;

#[derive(Clone, Debug)]
pub struct Track {
    pub title: Option<String>,
    pub url: String,
    pub is_notification: bool,
}

impl Track {
    pub fn notification(url: String) -> Self {
        Self {
            title: None,
            url,
            is_notification: true,
        }
    }
}

/// A station in rradio
#[derive(Clone, Debug)]
pub enum Station {
    UrlList {
        index: String,
        title: Option<String>,
        tracks: Vec<Track>,
    },
    Server {
        index: String,
        title: Option<String>,
        remote_addresses: Vec<String>,
    },
    CD {
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
    /// This function is only used by the `web_interface` feature.
    #[cfg(feature = "web_interface")]
    pub fn singleton(url: String) -> Self {
        Self::Singleton {
            track: Track {
                title: None,
                url,
                is_notification: false,
            },
        }
    }

    pub fn tracks(&self) -> Result<Vec<Track>> {
        match self {
            Self::UrlList { tracks, .. } => Ok(tracks.clone()),
            Self::Server { .. } => anyhow::bail!("Server not supported yet"),
            Self::CD { .. } => anyhow::bail!("CD not supported yet"),
            Self::Singleton { track } => Ok(vec![track.clone()]),
        }
    }
}
