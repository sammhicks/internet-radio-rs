//! A radio station in rradio

use std::path::Path;

use anyhow::{Context, Error, Result};

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
pub struct Station {
    /// The index which the user enters to select this station.
    /// Stations created on the fly have no index
    index: Option<String>,
    tracks: Vec<Track>,
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
                    "m3u" => Self::parse_m3u(path, index),
                    "pls" => Self::parse_pls(path, index),
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

    /// Parse an [M3U playlist](https://en.wikipedia.org/wiki/M3U)
    fn parse_m3u(path: impl AsRef<std::path::Path> + Clone, index: String) -> Result<Self> {
        use m3u::EntryExtReaderConstructionError;
        // First try parsing an extended M3U
        let maybe_items = match m3u::Reader::open_ext(path.clone()) {
            Ok(mut reader) => reader
                .entry_exts()
                .map(|entry| {
                    let entry = entry?;
                    Ok(Track {
                        title: Some(entry.extinf.name),
                        url: m3u_entry_url(entry.entry)?,
                        is_notification: false,
                    })
                })
                .collect(),
            Err(EntryExtReaderConstructionError::HeaderNotFound) => {
                // Not extended M3U, try normal M3U
                let mut reader = m3u::Reader::open(path)?;

                reader
                    .entries()
                    .map(|entry| {
                        Ok(Track {
                            title: None,
                            url: m3u_entry_url(entry?)?,
                            is_notification: false,
                        })
                    })
                    .collect()
            }
            Err(EntryExtReaderConstructionError::BufRead(err)) => {
                Err(err).context("Failed to read file")
            }
        };
        Ok(Self {
            index: Some(index),
            tracks: maybe_items?,
        })
    }

    /// Parse a [PLS playlist](https://en.wikipedia.org/wiki/PLS_(file_format))
    fn parse_pls(path: impl AsRef<std::path::Path>, index: String) -> Result<Self> {
        let mut reader = std::fs::File::open(path)?;
        let maybe_items = pls::parse(&mut reader)
            .map(|entries| {
                entries
                    .into_iter()
                    .map(|entry| Track {
                        title: entry.title,
                        url: entry.path,
                        is_notification: false,
                    })
                    .collect()
            })
            .map_err(Error::new);
        Ok(Self {
            index: Some(index),
            tracks: maybe_items?,
        })
    }

    /// Create a station consisting of a single url.
    /// This function is only used by the `web_interface` feature.
    #[cfg(feature = "web_interface")]
    pub fn singleton(url: String) -> Self {
        Self {
            index: None,
            tracks: vec![Track {
                title: None,
                url,
                is_notification: false,
            }],
        }
    }

    pub fn tracks(self) -> Vec<Track> {
        self.tracks
    }
}

fn m3u_entry_url(entry: m3u::Entry) -> Result<String> {
    Ok(match entry {
        m3u::Entry::Path(path) => String::from(
            path.to_str()
                .with_context(|| format!("Bad Path: {:?}", path))?,
        ),
        m3u::Entry::Url(url) => url.into_string(),
    })
}
