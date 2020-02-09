pub type ChannelIndex = u8;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Channel {
    pub name: String,
    #[serde(rename = "channel")]
    pub index: u8,
    pub url: String,
}
