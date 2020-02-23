use anyhow::{Context, Result};
use glib::{value::SendValue, StaticType, Type, Value};

#[derive(Debug)]
pub enum Tag {
    Title(String),
    Artist(String),
    Album(String),
    Genre(String),
    Unknown { name: String, value: String },
}

impl Tag {
    pub fn from_value(name: &str, value: &SendValue) -> Result<Self> {
        match name {
            "title" => get_value(value, Self::Title),
            "artist" => get_value(value, Self::Artist),
            "album" => get_value(value, Self::Album),
            "genre" => get_value(value, Self::Genre),
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
        Type::F64 => Ok(format!("F64: {}", value.get_some::<f64>()?)),
        t if t == gstreamer::DateTime::static_type() => value
            .get::<gstreamer::DateTime>()?
            .context("No DateTime")
            .map(|dt| format!("DateTime: {}", dt)),
        t if t == gstreamer::sample::Sample::static_type() => Ok(format!(
            "Sample: {:?}",
            value
                .get::<gstreamer::sample::Sample>()?
                .context("No Sample")?
        )),
        t => Ok(format!("Value of unhandled type {}: {:?}", t, value)),
    }
}
