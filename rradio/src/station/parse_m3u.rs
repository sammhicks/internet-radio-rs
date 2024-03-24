pub struct Track {
    title: Option<rradio_messages::ArcStr>,
    url: String,
}

impl From<Track> for super::Track {
    fn from(Track { title, url }: Track) -> Self {
        Self {
            title,
            album: None,
            artist: None,
            url: url.into(),
            is_notification: false,
        }
    }
}

/// [`super::StationLoader`] for an [M3U playlist](https://en.wikipedia.org/wiki/M3U)
pub struct Loader {
    pub path: std::path::PathBuf,
}

impl super::StationLoader for Loader {
    type Metadata = ();
    type Handle = ();
    type Track = Track;
    type Error = super::BadStationFile;

    const STATION_TYPE: rradio_messages::StationType = rradio_messages::StationType::UrlList;

    async fn load_station_parts(
        self,
        _: Option<()>,
        _: impl FnOnce(super::PartialInfo),
    ) -> Result<(Option<super::StationTitle>, Vec<Track>, (), ()), super::BadStationFile> {
        let Self { path } = self;

        let playlist_text =
            std::fs::read_to_string(&path).map_err(|error| super::BadStationFile {
                error: anyhow::Error::new(error)
                    .context(format!("Failed to read {}", path.display())),
            })?;

        let (station_title, tracks) =
            from_str(&playlist_text).map_err(|error| super::BadStationFile { error })?;

        Ok((station_title, tracks, (), ()))
    }
}

fn from_str(src: &str) -> anyhow::Result<(Option<super::StationTitle>, Vec<Track>)> {
    use anyhow::Context;

    let lines = src.lines().map(str::trim).filter(|line| !line.is_empty());

    if src.starts_with("#EXTM3U") {
        let mut lines = lines.enumerate();

        let mut title = None;

        let tracks = std::iter::from_fn(|| loop {
            let (line_num, line) = lines.next()?;

            if let Some(playlist) = line.strip_prefix("#PLAYLIST:") {
                title = Some(super::StationTitle {
                    station_title: playlist.trim().into(),
                });

                continue;
            }

            if let Some(extra_info) = line.strip_prefix("#EXTINF:") {
                let title = match extra_info
                    .split_once(',')
                    .with_context(|| format!("Badly formatted EXTINF on line {line_num}"))
                {
                    Ok((_, title)) => Some(title.trim().into()),
                    Err(err) => return Some(Err(err)),
                };

                let url = match lines
                    .find(|(_, line)| !line.starts_with('#'))
                    .with_context(|| format!("No url after EXTINF on line {line_num}"))
                {
                    Ok((_, url)) => url.trim().into(),
                    Err(err) => return Some(Err(err)),
                };

                return Some(Ok(Track { title, url }));
            }

            if !line.starts_with('#') {
                return Some(Ok(Track {
                    title: None,
                    url: line.into(),
                }));
            }
        })
        .collect::<anyhow::Result<_>>()?;

        Ok((title, tracks))
    } else {
        let tracks = lines
            .filter(|line| !line.starts_with('#'))
            .map(|url| Track {
                title: None,
                url: url.into(),
            })
            .collect();

        Ok((None, tracks))
    }
}

#[cfg(test)]
mod tests {
    use super::{from_str, Track};

    fn verify_track(title: Option<&str>, url: &str, track: &Track) {
        assert_eq!(track.title.as_deref(), title);
        assert_eq!(track.url.as_str(), url);
    }

    fn verify_station<const N: usize>(
        (title, tracks): (Option<super::super::StationTitle>, Vec<Track>),
        test_title: Option<&str>,
        test_tracks: [fn(&Track); N],
    ) {
        assert_eq!(
            title.as_ref().map(|title| title.station_title.as_str()),
            test_title
        );

        assert_eq!(tracks.len(), test_tracks.len());

        for (track, test_track) in tracks.iter().zip(IntoIterator::into_iter(test_tracks)) {
            test_track(track);
        }
    }

    #[test]
    fn empty_file() {
        verify_station(from_str("").unwrap(), None, []);
    }

    #[test]
    fn m3u_file() {
        verify_station(
            from_str("a\nb\n\nc\n").unwrap(),
            None,
            [
                |track| verify_track(None, "a", track),
                |track| verify_track(None, "b", track),
                |track| verify_track(None, "c", track),
            ],
        );
    }

    #[test]
    fn extm3u_file() {
        verify_station(
            from_str(
                "#EXTM3U\n#PLAYLIST: P\n#EXTINF:-1, A\na\n#EXTINF:-1, B\n\nb\n\n#EXTINF:-1, C\nc\n",
            )
            .unwrap(),
            Some("P"),
            [
                |track| verify_track(Some("A"), "a", track),
                |track| verify_track(Some("B"), "b", track),
                |track| verify_track(Some("C"), "c", track),
            ],
        );
    }

    #[test]
    fn extm3u_file_extinf_missing() {
        verify_station(
            from_str("#EXTM3U\n#EXTINF:-1, A\na\n\n\nb\n\n#EXTINF:-1, C\nc\n").unwrap(),
            None,
            [
                |track| verify_track(Some("A"), "a", track),
                |track| verify_track(None, "b", track),
                |track| verify_track(Some("C"), "c", track),
            ],
        );
    }
}
