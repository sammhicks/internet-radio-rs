use std::path::{Path, PathBuf};

use rradio_messages::StationIndex;

use anyhow::{Context, Result};
use rand::{prelude::SliceRandom, Rng};
use url::Url;

use super::Track;

mod container;
mod root_description;

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum SortBy {
    None,
    TrackNumber,
    Random,
}

impl Default for SortBy {
    fn default() -> Self {
        Self::None
    }
}

#[derive(Debug, serde::Deserialize)]
struct Container {
    #[serde(default)]
    station_title: Option<String>,
    root_description_url: Url,
    container: PathBuf,
    #[serde(default)]
    sort_by: SortBy,
    #[serde(default)]
    limit_track_count: Option<usize>,
    #[serde(default)]
    filter_upnp_class: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct ContainerEnvelope {
    container: Container,
}

#[derive(Debug, serde::Deserialize)]
struct RandomContainerEnvelope {
    random_container: Container,
}

#[derive(Debug, serde::Deserialize)]
struct FlattenedContainerEnvelope {
    flattened_container: Container,
}

#[derive(Debug, serde::Deserialize)]
#[serde(untagged)]
enum Envelope {
    Single(ContainerEnvelope),
    Random(RandomContainerEnvelope),
    Flattened(FlattenedContainerEnvelope),
}

impl Envelope {
    fn container(&self) -> &Container {
        match self {
            Envelope::Single(ContainerEnvelope { container })
            | Envelope::Random(RandomContainerEnvelope {
                random_container: container,
            })
            | Envelope::Flattened(FlattenedContainerEnvelope {
                flattened_container: container,
            }) => container,
        }
    }
}

struct RootContainerBuilder {
    client: reqwest::Client,
    root_device: root_description::DeviceInfo,
    current_container: container::Container,
}

impl RootContainerBuilder {
    async fn new(root_description_url: Url) -> Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent("rradio")
            .build()
            .context("Failed to create http client")?;

        let root_device =
            root_description::get_content_directory_control_path(&client, root_description_url)
                .await?;

        let current_container = container::fetch(
            &client,
            &root_device.content_directory_control_url,
            container::Reference {
                id: "0".into(),
                title: "<root>".into(),
            },
        )
        .await?;

        Ok(Self {
            client,
            root_device,
            current_container,
        })
    }

    async fn with_container_path(mut self, container_path: &Path) -> Result<Self> {
        for section in container_path {
            let section = section.to_str().context("Bad path")?;

            let title = self.current_container.title;

            let reference = self
                .current_container
                .containers
                .into_iter()
                .find(|container| container.title == section)
                .with_context(|| format!("Container {:?} not found in {:?}", section, title))?;

            self.current_container = container::fetch(
                &self.client,
                &self.root_device.content_directory_control_url,
                reference,
            )
            .await?;
        }

        Ok(self)
    }

    async fn random_subcontainer(self) -> Result<TracksBuilder> {
        let mut current_container = self.current_container;
        let items = loop {
            if !current_container.items.is_empty() {
                break current_container.items;
            }

            if current_container.containers.is_empty() {
                anyhow::bail!("Container contains no containers");
            }

            let reference = current_container
                .containers
                .remove(rand::thread_rng().gen_range(0..current_container.containers.len()));

            current_container = container::fetch(
                &self.client,
                &self.root_device.content_directory_control_url,
                reference,
            )
            .await?;
        };

        Ok(TracksBuilder { items })
    }

    async fn flatten_container(self) -> Result<TracksBuilder> {
        let mut items = self.current_container.items;
        let mut containers = self.current_container.containers;

        while let Some(container) = containers.pop() {
            let mut new_container = container::fetch(
                &self.client,
                &self.root_device.content_directory_control_url,
                container,
            )
            .await?;

            items.append(&mut new_container.items);
            containers.append(&mut new_container.containers);
        }

        Ok(TracksBuilder { items })
    }

    async fn tracks(self, envelope: &Envelope) -> Result<TracksBuilder> {
        match envelope {
            Envelope::Single(_) => Ok(TracksBuilder {
                items: self.current_container.items,
            }),
            Envelope::Random(_) => self.random_subcontainer().await,
            Envelope::Flattened(_) => self.flatten_container().await,
        }
    }
}

struct TracksBuilder {
    items: Vec<container::Item>,
}

impl TracksBuilder {
    fn filter_upnp_class(mut self, filter_upnp_class: Option<&str>) -> Self {
        if let Some(filter_upnp_class) = filter_upnp_class {
            let filter_upnp_class = filter_upnp_class.trim();

            self.items
                .retain(|item| item.upnp_class.trim() == filter_upnp_class);
        }

        self
    }

    fn sort_tracks(mut self, sort_by: SortBy) -> Self {
        match sort_by {
            SortBy::None => (),
            SortBy::TrackNumber => self.items.sort_by_key(|item| item.track_number),
            SortBy::Random => self.items.shuffle(&mut rand::thread_rng()),
        }
        self
    }

    fn limit_track_count(mut self, limit_track_count: Option<usize>) -> Self {
        if let Some(limit_track_count) = limit_track_count {
            self.items.truncate(limit_track_count);
        }

        self
    }

    fn tracks(self) -> Vec<Track> {
        self.items.into_iter().map(Track::from).collect()
    }
}

#[derive(Clone)]
struct Metadata {
    station_index: Option<StationIndex>,
    station_title: Option<String>,
    tracks: Vec<Track>,
}

impl Metadata {
    fn into_playlist(self) -> super::Playlist {
        let Metadata {
            station_index,
            station_title,
            tracks,
        } = self.clone();

        super::Playlist {
            station_index,
            station_title,
            station_type: rradio_messages::StationType::UPnP,
            tracks,
            metadata: super::PlaylistMetadata::new(self),
            handle: super::PlaylistHandle::default(),
        }
    }
}

#[derive(Debug)]
pub struct Station {
    index: StationIndex,
    envelope: Envelope,
}

impl Station {
    pub fn from_file(path: &std::path::Path, index: StationIndex) -> Result<Self> {
        tracing::trace!("parsing upnp playlist");

        let file = std::fs::read_to_string(path)
            .with_context(|| format!(r#"Failed to read "{}""#, path.display()))?;

        let envelope = toml::from_str(&file)
            .with_context(|| format!(r#"Failed to parse "{}""#, path.display()))?;

        Ok(Self { index, envelope })
    }

    pub fn index(&self) -> &StationIndex {
        &self.index
    }

    pub fn title(&self) -> Option<&str> {
        self.envelope.container().station_title.as_deref()
    }

    pub async fn into_playlist(
        self,
        metadata: Option<&super::PlaylistMetadata>,
    ) -> Result<super::Playlist> {
        if let Some(metadata) = metadata
            .and_then(|super::PlaylistMetadata(metadata)| metadata.downcast_ref::<Metadata>())
            .cloned()
        {
            return Ok(metadata.into_playlist());
        }

        let station_index = Some(self.index);
        let station_title = self.envelope.container().station_title.clone();
        let tracks =
            RootContainerBuilder::new(self.envelope.container().root_description_url.clone())
                .await?
                .with_container_path(&self.envelope.container().container)
                .await?
                .tracks(&self.envelope)
                .await?
                .filter_upnp_class(self.envelope.container().filter_upnp_class.as_deref())
                .sort_tracks(self.envelope.container().sort_by)
                .limit_track_count(self.envelope.container().limit_track_count)
                .tracks();

        let metadata = Metadata {
            station_index: station_index.clone(),
            station_title: station_title.clone(),
            tracks: tracks.clone(),
        };

        Ok(super::Playlist {
            station_index,
            station_title,
            station_type: rradio_messages::StationType::UPnP,
            tracks,
            metadata: super::PlaylistMetadata::new(metadata),
            handle: super::PlaylistHandle::default(),
        })
    }
}

/// Parse a `UPnP` Station
pub fn from_file(path: &std::path::Path, index: StationIndex) -> Result<super::Station> {
    Station::from_file(path, index).map(super::Station::UPnP)
}
