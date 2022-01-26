use anyhow::{Context, Result};

#[derive(serde::Deserialize)]
struct Root {
    device: Device,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Device {
    friendly_name: String,
    service_list: Services,
}

#[derive(serde::Deserialize)]
struct Services {
    service: Vec<Service>,
}

#[derive(serde::Deserialize)]
struct Service {
    #[serde(rename = "serviceType")]
    service_type: String,
    #[serde(rename = "controlURL")]
    control_url: String,
}

pub struct DeviceInfo {
    pub name: String,
    pub content_directory_control_url: String,
}

pub async fn get_content_directory_control_path(
    client: &reqwest::Client,
    url: url::Url,
) -> Result<DeviceInfo> {
    log::trace!("Fetching {}", url.as_str());

    let root_description = client
        .get(url.clone())
        .send()
        .await
        .context("Failed to fetch root description")?
        .bytes()
        .await
        .context("Failed to fetch root description text")?;

    let root = quick_xml::de::from_reader::<_, Root>(root_description.as_ref())
        .context("Failed to parse root description")?;

    let content_directory_control_path = root
        .device
        .service_list
        .service
        .into_iter()
        .find_map(|service| {
            (service.service_type == "urn:schemas-upnp-org:service:ContentDirectory:1")
                .then(|| service.control_url)
        })
        .context("Content Directory Service not found")?;

    let mut content_directory_url = url;

    content_directory_url.set_path(&content_directory_control_path);
    content_directory_url.set_query(None);
    content_directory_url.set_fragment(None);

    Ok(DeviceInfo {
        name: root.device.friendly_name,
        content_directory_control_url: content_directory_url.as_str().into(),
    })
}
