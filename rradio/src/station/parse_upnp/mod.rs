use std::path::PathBuf;

use anyhow::{Context, Result};
use rand::{prelude::SliceRandom, Rng};
use url::Url;

use super::{Station, Track};

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

pub async fn parse(path: impl AsRef<std::path::Path> + Copy, index: String) -> Result<Station> {
    log::trace!("parsing upnp playlist");

    let file = std::fs::read_to_string(path)
        .with_context(|| format!(r#"Failed to read "{}""#, path.as_ref().display()))?;

    let envelope = toml::from_str::<Envelope>(&file)
        .with_context(|| format!(r#"Failed to parse "{}""#, path.as_ref().display()))?;

    log::debug!("Station: {:?}", envelope);

    let client = reqwest::Client::builder()
        .user_agent("rradio")
        .build()
        .context("Failed to create http client")?;

    let root_device = root_description::get_content_directory_control_path(
        &client,
        envelope.container().root_description_url.clone(),
    )
    .await?;

    let mut current_container = container::fetch(
        &client,
        &root_device.content_directory_control_url,
        container::Reference {
            id: "0".into(),
            title: "<root>".into(),
        },
    )
    .await?;

    for section in envelope.container().container.as_path() {
        let section = section.to_str().context("Bad path")?;

        let title = current_container.title;

        let reference = current_container
            .containers
            .into_iter()
            .find(|container| container.title == section)
            .with_context(|| format!("Container {:?} not found in {:?}", section, title))?;

        current_container = container::fetch(
            &client,
            &root_device.content_directory_control_url,
            reference,
        )
        .await?;
    }

    let mut items = match envelope {
        Envelope::Single(_) => current_container.items,
        Envelope::Random(_) => loop {
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
                &client,
                &root_device.content_directory_control_url,
                reference,
            )
            .await?;
        },
        Envelope::Flattened(_) => {
            let mut items = current_container.items;
            let mut containers = current_container.containers;

            while let Some(container) = containers.pop() {
                let mut new_container = container::fetch(
                    &client,
                    &root_device.content_directory_control_url,
                    container,
                )
                .await?;

                items.append(&mut new_container.items);
                containers.append(&mut new_container.containers);
            }

            items
        }
    };

    match envelope.container().sort_by {
        SortBy::None => (),
        SortBy::TrackNumber => items.sort_by_key(|item| item.track_number),
        SortBy::Random => items.shuffle(&mut rand::thread_rng()),
    }

    if let Some(limit_track_count) = envelope.container().limit_track_count {
        items.truncate(limit_track_count);
    }

    Ok(Station::UrlList {
        index,
        title: envelope
            .container()
            .station_title
            .clone()
            .or(Some(root_device.name)),
        tracks: items.into_iter().map(Track::from).collect(),
    })
}
