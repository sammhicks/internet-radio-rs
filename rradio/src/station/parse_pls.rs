use anyhow::{Error, Result};

use rradio_messages::{ArcStr, StationIndex};

use super::{Station, Track};

/// Parse a [PLS playlist](https://en.wikipedia.org/wiki/PLS_(file_format))
pub fn from_file(path: &std::path::Path, index: StationIndex) -> Result<Station> {
    let mut reader = std::fs::File::open(path)?;
    let maybe_tracks = pls::parse(&mut reader)
        .map(|entries| {
            entries
                .into_iter()
                .map(|entry| Track {
                    title: entry.title.map(ArcStr::from),
                    album: None,
                    artist: None,
                    url: entry.path.into(),
                    is_notification: false,
                })
                .collect()
        })
        .map_err(Error::new);
    Ok(Station::UrlList {
        index: Some(index),
        title: None,
        tracks: maybe_tracks?,
    })
}
