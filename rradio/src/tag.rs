//! Tracks have tags attached to them.

use anyhow::{Context, Result};
use glib::value::SendValue;

use rradio_messages::ArcStr;

/// The image tag of a track.
/// This wrapper is to avoid dumping to contents of an image to the terminal when debug printing a track tag.
pub struct Image(ArcStr);

impl Image {
    fn new(mime_type: &str, image_data: &[u8]) -> Self {
        Self(format!("data:{};base64,{}", mime_type, base64::encode(image_data)).into())
    }

    pub fn into_inner(self) -> ArcStr {
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
    Title(ArcStr),
    Organisation(ArcStr),
    Artist(ArcStr),
    Album(ArcStr),
    Genre(ArcStr),
    Image(Image),
    Comment(ArcStr),
    Unknown { name: ArcStr, value: ArcStr },
}

impl Tag {
    pub fn from_value(name: &str, value: &SendValue) -> Result<Self> {
        match name {
            "title" => get_atomic_string(value, Self::Title),
            "organisation" | "organization" => get_atomic_string(value, Self::Organisation),
            "artist" => get_atomic_string(value, Self::Artist),
            "album" => get_atomic_string(value, Self::Album),
            "genre" => get_atomic_string(value, Self::Genre),
            "image" => {
                let image = value.get::<gstreamer::Sample>().context("No Value")?;

                let image_buffer = image.buffer().context("No Buffer")?;
                let all_mem = image_buffer
                    .all_memory()
                    .context("Failed to get all memory")?;
                let readable_mem = all_mem.map_readable().context("Failed to read buffer")?;

                let caps = image.caps().context("No Caps")?;

                let mime_type = caps.structure(0).context("No Cap 0")?.name();

                Ok(Self::Image(Image::new(mime_type, readable_mem.as_slice())))
            }
            "comment" => get_atomic_string(value, Self::Comment),
            _ => Ok(Self::Unknown {
                name: name.into(),
                value: value_to_string(value)?.into(),
            }),
        }
    }
}

fn get_value<'v, T, F>(value: &'v SendValue, builder: F) -> Result<Tag>
where
    T: glib::value::FromValue<'v>,
    <T::Checker as glib::value::ValueTypeChecker>::Error: Sync,
    F: FnOnce(T) -> Tag,
{
    value.get().context("No Value").map(builder)
}

fn get_atomic_string<F: FnOnce(ArcStr) -> Tag>(value: &SendValue, builder: F) -> Result<Tag> {
    get_value(value, |str: &str| builder(ArcStr::from(str)))
}

pub fn value_to_string(value: &glib::value::Value) -> Result<String> {
    use glib::Type;
    let value_type = value.type_();

    Ok(if value_type.is_a(Type::BOOL) {
        format!("Bool: {}", value.get::<bool>()?)
    } else if value_type.is_a(Type::STRING) {
        format!("String: {}", value.get::<String>()?)
    } else if value_type.is_a(Type::U32) {
        format!("U32: {}", value.get::<u32>()?)
    } else if value_type.is_a(Type::U64) {
        format!("U64: {}", value.get::<u64>()?)
    } else if value_type.is_a(Type::F64) {
        format!("F64: {}", value.get::<f64>()?)
    } else {
        format!("Value of unhandled type {}: {:?}", value_type, value)
    })
}
