pub struct Track(pls::PlaylistElement);

impl From<Track> for super::Track {
    fn from(
        Track(pls::PlaylistElement {
            path,
            title,
            len: _,
        }): Track,
    ) -> Self {
        Self {
            title: title.map(Into::into),
            album: None,
            artist: None,
            url: path.into(),
            is_notification: false,
        }
    }
}

/// [`super::StationLoader`] for a [PLS playlist](https://en.wikipedia.org/wiki/PLS_(file_format))
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
        use anyhow::Context;

        let Self { path } = self;

        Ok((
            None,
            std::fs::File::open(&path)
                .context(format!("Failed to read {}", path.display()))
                .and_then(|mut data| {
                    pls::parse(&mut data).context(format!("Failed to parse {}", path.display()))
                })
                .map_err(|error| super::BadStationFile { error })?
                .into_iter()
                .map(Track)
                .collect(),
            (),
            (),
        ))
    }
}
