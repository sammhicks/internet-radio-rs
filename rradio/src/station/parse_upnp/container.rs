use anyhow::{Context, Result};
use rradio_messages::ArcStr;

fn map_into<A, B: From<A>>(a: Option<A>) -> Option<B> {
    a.map(B::from)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SoapEnvelope {
    body: SoapBody,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct SoapBody {
    browse_response: BrowseResponse,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
struct BrowseResponse {
    result: BrowseResponseResult,
}

#[derive(serde::Deserialize)]
struct BrowseResponseResult {
    #[serde(rename = "$value")]
    body: String,
}

#[derive(serde::Deserialize)]
struct DidlRoot {
    #[serde(rename = "container", default)]
    containers: Vec<Reference>,
    #[serde(rename = "item", default)]
    items: Vec<Item>,
}

#[derive(Debug, serde::Deserialize)]
pub struct Reference {
    pub id: String,
    pub title: String,
}

#[derive(serde::Deserialize)]
struct ItemDerive {
    #[serde(rename = "originalTrackNumber")]
    track_number: usize,
    title: Vec<String>,
    album: Vec<String>,
    artist: Vec<String>,
    #[serde(rename = "res")]
    urls: Vec<String>,
}

#[derive(Debug)]
pub struct Item {
    pub track_number: usize,
    pub title: Option<ArcStr>,
    pub album: Option<ArcStr>,
    pub artist: Option<ArcStr>,
    pub url: ArcStr,
}

impl<'de> serde::Deserialize<'de> for Item {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error;

        let item = ItemDerive::deserialize(deserializer)?;

        Ok(Self {
            track_number: item.track_number,
            title: map_into(item.title.into_iter().next()),
            album: map_into(item.album.into_iter().next()),
            artist: map_into(item.artist.into_iter().next()),
            url: item
                .urls
                .into_iter()
                .next()
                .ok_or_else(|| D::Error::missing_field("res"))?
                .into(),
        })
    }
}

pub struct Container {
    pub title: String,
    pub containers: Vec<Reference>,
    pub items: Vec<Item>,
}

pub async fn fetch(
    client: &reqwest::Client,
    control_url: &str,
    Reference { id, title }: Reference,
) -> Result<Container> {
    log::trace!("Fetching {}", id);

    let body = include_str!("request.xml").replace("%%%ObjectID%%%", &id);

    let http_response = client
        .post(control_url)
        .header(reqwest::header::CONTENT_TYPE, "text/xml;charset=utf-8")
        .header(
            "Soapaction",
            "urn:schemas-upnp-org:service:ContentDirectory:1#Browse",
        )
        .body(body)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .context("Failed to fetch container")?
        .text()
        .await
        .context("Failed to fetch text")?;

    log::trace!("Parsing XML");

    let browse_result = quick_xml::de::from_str::<SoapEnvelope>(&http_response)
        .context("Failed to parse Soap Envelope")?
        .body
        .browse_response
        .result
        .body;

    let DidlRoot { containers, items } = quick_xml::de::from_str::<DidlRoot>(&browse_result)
        .context("Failed to parse Soap Payload")?;

    Ok(Container {
        title,
        containers,
        items,
    })
}
