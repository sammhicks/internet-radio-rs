use anyhow::{Error, Result};

use super::{Station, Track};

/// Parse a [PLS playlist](https://en.wikipedia.org/wiki/PLS_(file_format))
pub fn parse(path: impl AsRef<std::path::Path>, index: String) -> Result<Station> {
    let mut reader = std::fs::File::open(path)?;
    let maybe_tracks = pls::parse(&mut reader)
        .map(|entries| {
            entries
                .into_iter()
                .map(|entry| Track {
                    title: entry.title,
                    album: None,
                    artist: None,
                    url: entry.path,
                    is_notification: false,
                })
                .collect()
        })
        .map_err(Error::new);
    Ok(Station::UrlList {
        index,
        title: None,
        pause_before_playing: None,
        show_buffer: None,
        tracks: maybe_tracks?,
    })
}
