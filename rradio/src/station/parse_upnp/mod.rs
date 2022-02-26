use std::path::{Path, PathBuf};

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
    root_description_url: Url,
    container: PathBuf,
    #[serde(default)]
    sort_by: SortBy,
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
    fn root_description_url(&self) -> &Url {
        match self {
            Self::Single(ContainerEnvelope {
                container:
                    Container {
                        root_description_url,
                        ..
                    },
            })
            | Self::Random(RandomContainerEnvelope {
                random_container:
                    Container {
                        root_description_url,
                        ..
                    },
            })
            | Self::Flattened(FlattenedContainerEnvelope {
                flattened_container:
                    Container {
                        root_description_url,
                        ..
                    },
            }) => root_description_url,
        }
    }

    fn container_path(&self) -> &Path {
        match self {
            Self::Single(ContainerEnvelope {
                container: Container { container, .. },
            })
            | Self::Random(RandomContainerEnvelope {
                random_container: Container { container, .. },
            })
            | Self::Flattened(FlattenedContainerEnvelope {
                flattened_container: Container { container, .. },
            }) => container.as_path(),
        }
    }

    fn sort_by(&self) -> SortBy {
        match *self {
            Self::Single(ContainerEnvelope {
                container: Container { sort_by, .. },
            })
            | Self::Random(RandomContainerEnvelope {
                random_container: Container { sort_by, .. },
            })
            | Self::Flattened(FlattenedContainerEnvelope {
                flattened_container: Container { sort_by, .. },
            }) => sort_by,
        }
    }
}

pub async fn parse(path: impl AsRef<std::path::Path> + Copy, index: String) -> Result<Station> {
    log::trace!("parsing upnp playlist");

    let file = std::fs::read_to_string(path)
        .with_context(|| format!(r#"Failed to read "{}""#, path.as_ref().display()))?;

    let envelope = toml::from_str::<Envelope>(&file)
        .with_context(|| format!(r#"Failed to parse "{}""#, path.as_ref().display()))?;

    let client = reqwest::Client::builder()
        .user_agent("rradio")
        .build()
        .context("Failed to create http client")?;

    let root_device = root_description::get_content_directory_control_path(
        &client,
        envelope.root_description_url().clone(),
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

    for section in envelope.container_path() {
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

            log::trace!("Selecting container {:?}", reference.title);

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

    match envelope.sort_by() {
        SortBy::None => (),
        SortBy::TrackNumber => items.sort_by_key(|item| item.track_number),
        SortBy::Random => items.shuffle(&mut rand::thread_rng()),
    }

    Ok(Station::UrlList {
        index,
        title: Some(root_device.name),
        tracks: items.into_iter().map(Track::from).collect(),
    })
}
