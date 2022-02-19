use anyhow::{Context, Result};

use super::{Station, Track};

/// Parse an [M3U playlist](https://en.wikipedia.org/wiki/M3U)
pub fn parse(path: impl AsRef<std::path::Path> + Copy, index: String) -> Result<Station> {
    let playlist_text = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.as_ref().display()))?;

    parse_from_str(&playlist_text, index)
}

fn parse_from_str(src: &str, index: String) -> Result<Station> {
    let lines = src.lines().map(str::trim).filter(|line| !line.is_empty());

    if src.starts_with("#EXTM3U") {
        let mut lines = lines.enumerate();

        let mut title = None;

        let tracks = std::iter::from_fn(|| loop {
            let (line_num, line) = lines.next()?;

            if let Some(playlist) = line.strip_prefix("#PLAYLIST:") {
                title = Some(String::from(playlist.trim()));
                continue;
            }

            if let Some(extra_info) = line.strip_prefix("#EXTINF:") {
                let title = match extra_info
                    .split_once(',')
                    .with_context(|| format!("Badly formatted EXTINF on line {}", line_num))
                {
                    Ok((_, title)) => Some(title.trim().into()),
                    Err(err) => return Some(Err(err)),
                };

                let url = match (&mut lines)
                    .find(|(_, line)| !line.starts_with('#'))
                    .with_context(|| format!("No url after EXTINF on line {}", line_num))
                {
                    Ok((_, url)) => url.trim().into(),
                    Err(err) => return Some(Err(err)),
                };

                return Some(Ok(Track {
                    title,
                    album: None,
                    artist: None,
                    url,
                    is_notification: false,
                }));
            }

            if !line.starts_with('#') {
                return Some(Ok(Track {
                    title: None,
                    album: None,
                    artist: None,
                    url: line.into(),
                    is_notification: false,
                }));
            }
        })
        .collect::<Result<_>>()?;

        Ok(Station::UrlList {
            index,
            title,
            tracks,
        })
    } else {
        let tracks = lines
            .filter(|line| !line.starts_with('#'))
            .map(|url| Track {
                title: None,
                album: None,
                artist: None,
                url: url.into(),
                is_notification: false,
            })
            .collect();

        Ok(Station::UrlList {
            index,
            title: None,
            tracks,
        })
    }
}

#[cfg(test)]
mod tests {
    use rradio_messages::Track;

    use super::{parse_from_str, Station};

    const INDEX: &str = "42";

    fn verify_track(title: Option<&str>, url: &str, track: &Track) {
        assert_eq!(track.title.as_deref(), title);
        assert_eq!(track.album, None);
        assert_eq!(track.artist, None);
        assert_eq!(track.url.as_str(), url);
        assert!(!track.is_notification);
    }

    fn verify_station<const N: usize>(
        station: Station,
        test_title: Option<&str>,
        test_tracks: [fn(&Track); N],
    ) {
        if let Station::UrlList {
            index,
            title,
            tracks,
        } = station
        {
            assert_eq!(index, INDEX);
            assert_eq!(title.as_deref(), test_title);

            assert_eq!(tracks.len(), test_tracks.len());

            for (track, test_track) in tracks.iter().zip(IntoIterator::into_iter(test_tracks)) {
                test_track(track);
            }
        } else {
            panic!("Expected UrlList, found {:?}", station);
        }
    }

    #[test]
    fn empty_file() {
        verify_station(parse_from_str("", String::from(INDEX)).unwrap(), None, []);
    }

    #[test]
    fn m3u_file() {
        verify_station(
            parse_from_str("a\nb\n\nc\n", String::from(INDEX)).unwrap(),
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
            parse_from_str(
                "#EXTM3U\n#PLAYLIST: P\n#EXTINF:-1, A\na\n#EXTINF:-1, B\n\nb\n\n#EXTINF:-1, C\nc\n",
                String::from(INDEX),
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
            parse_from_str(
                "#EXTM3U\n#EXTINF:-1, A\na\n\n\nb\n\n#EXTINF:-1, C\nc\n",
                String::from(INDEX),
            )
            .unwrap(),
            None,
            [
                |track| verify_track(Some("A"), "a", track),
                |track| verify_track(None, "b", track),
                |track| verify_track(Some("C"), "c", track),
            ],
        );
    }
}
