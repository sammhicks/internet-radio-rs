use anyhow::{Context, Result};

use super::{Station, Track};

/// Parse an [M3U playlist](https://en.wikipedia.org/wiki/M3U)
pub fn parse(path: impl AsRef<std::path::Path> + Clone, index: String) -> Result<Station> {
    use m3u::EntryExtReaderConstructionError;
    // First try parsing an extended M3U
    let maybe_tracks = match m3u::Reader::open_ext(path.clone()) {
        Ok(mut reader) => reader
            .entry_exts()
            .map(|entry| {
                let entry = entry?;
                Ok(Track {
                    title: Some(entry.extinf.name),
                    album: None,
                    artist: None,
                    url: m3u_entry_url(entry.entry)?,
                    is_notification: false,
                })
            })
            .collect::<Result<_>>(),
        Err(EntryExtReaderConstructionError::HeaderNotFound) => {
            // Not extended M3U, try normal M3U
            let mut reader = m3u::Reader::open(path)?;

            reader
                .entries()
                .map(|entry| {
                    Ok(Track {
                        title: None,
                        album: None,
                        artist: None,
                        url: m3u_entry_url(entry?)?,
                        is_notification: false,
                    })
                })
                .collect::<Result<_>>()
        }
        Err(EntryExtReaderConstructionError::BufRead(err)) => {
            Err(err).context("Failed to read file")
        }
    };

    Ok(Station::UrlList {
        index,
        title: None,
        pause_before_playing: None,
        show_buffer: None,
        tracks: maybe_tracks?,
    })
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
