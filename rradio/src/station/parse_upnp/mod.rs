use anyhow::{Context, Result};

use super::{Station, Track};

mod container;
mod root_description;

pub async fn parse(path: impl AsRef<std::path::Path> + Copy, index: String) -> Result<Station> {
    log::trace!("parsing upnp playlist");

    let file = std::fs::read_to_string(path)
        .with_context(|| format!(r#"Failed to read "{}""#, path.as_ref().display()))?;
    let mut lines = file.lines().map(str::trim).filter(|line| !line.is_empty());

    let root_description_url = lines.next().context("Root Description line is missing")?;
    let path = lines.next().context("Path line missing")?;

    log::trace!("root description url: {}", root_description_url);
    log::trace!("path: {}", root_description_url);

    let client = reqwest::Client::builder()
        .user_agent("rradio")
        .build()
        .context("Failed to create http client")?;

    let root_device =
        root_description::get_content_directory_control_path(&client, root_description_url).await?;

    let mut current_container = container::fetch(
        &client,
        &root_device.content_directory_control_url,
        container::Reference {
            id: "0".into(),
            title: "<root>".into(),
        },
    )
    .await?;

    for section in std::path::Path::new(path) {
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

    Ok(Station::UrlList {
        index,
        title: Some(root_device.name),
        pause_before_playing: None,
        show_buffer: None,
        tracks: current_container
            .items
            .into_iter()
            .map(
                |container::Item {
                     title,
                     album,
                     artist,
                     url,
                 }| Track {
                    title,
                    album,
                    artist,
                    url,
                    is_notification: false,
                },
            )
            .collect(),
    })
}
