//! Tracks have tags attached to them.

use anyhow::{Context, Result};
use glib::{value::SendValue, StaticType, Type, Value};

/// The image tag of a track.
/// This wrapper is to avoid dumping to contents of an image to the terminal when debug printing a track tag.
pub struct Image(String);

impl Image {
    fn new(mime_type: &str, image_data: &[u8]) -> Self {
        Self(format!(
            "data:{};base64,{}",
            mime_type,
            base64::encode(image_data)
        ))
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::fmt::Debug for Image {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<image>")
    }
}

/// A tag attached to a track
#[derive(Debug)]
pub enum Tag {
    Title(String),
    Organisation(String),
    Artist(String),
    Album(String),
    Genre(String),
    Image(Image),
    Comment(String),
    Unknown { name: String, value: String },
}

impl Tag {
    pub fn from_value(name: &str, value: &SendValue) -> Result<Self> {
        match name {
            "title" => get_value(value, Self::Title),
            "organisation" | "organization" => get_value(value, Self::Organisation),
            "artist" => get_value(value, Self::Artist),
            "album" => get_value(value, Self::Album),
            "genre" => get_value(value, Self::Genre),
            "image" => {
                let image = value.get::<gstreamer::Sample>()?.context("No Value")?;

                let image_buffer = image.get_buffer().context("No Buffer")?;
                let all_mem = image_buffer
                    .get_all_memory()
                    .context("Failed to get all memory")?;
                let readable_mem = all_mem.map_readable().context("Failed to read buffer")?;

                let caps = image.get_caps().context("No Caps")?;

                let mime_type = caps.get_structure(0).context("No Cap 0")?.get_name();

                Ok(Self::Image(Image::new(mime_type, readable_mem.as_slice())))
            }
            "comment" => get_value(value, Self::Comment),
            _ => Ok(Self::Unknown {
                name: name.into(),
                value: value_to_string(value)?,
            }),
        }
    }
}

fn get_value<'v, T, F>(value: &'v SendValue, builder: F) -> Result<Tag>
where
    T: glib::value::FromValueOptional<'v>,
    F: FnOnce(T) -> Tag,
{
    value.get()?.context("No Value").map(builder)
}

pub fn value_to_string(value: &Value) -> Result<String> {
    match value.type_() {
        Type::Bool => value
            .get::<bool>()?
            .context("No Bool")
            .map(|b| format!("Bool: {}", b)),
        Type::String => value
            .get::<String>()?
            .context("No String")
            .map(|s| format!("String: {}", s)),
        Type::U32 => Ok(format!("U32: {}", value.get_some::<u32>()?)),
        Type::U64 => Ok(format!("U64: {}", value.get_some::<u64>()?)),
        Type::F64 => Ok(format!("F64: {}", value.get_some::<f64>()?)),
        t if t == gstreamer::DateTime::static_type() => value
            .get::<gstreamer::DateTime>()?
            .context("No DateTime")
            .map(|dt| format!("DateTime: {}", dt)),
        t if t == gstreamer::sample::Sample::static_type() => Ok(format!(
            "Sample: {:?}",
            value.get::<gstreamer::Sample>()?.context("No Sample")?
        )),
        t => Ok(format!("Value of unhandled type {}: {:?}", t, value)),
    }
}
