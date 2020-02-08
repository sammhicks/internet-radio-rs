use anyhow::{Error, Result};
use glib::{StaticType, Type, Value};

pub fn value_to_string(value: &Value) -> Result<String> {
    match value.type_() {
        Type::String => value
            .get::<String>()?
            .ok_or(Error::msg("No String"))
            .map(|s| format!("String: {}", s)),
        Type::U32 => Ok(format!("U32: {}", value.get_some::<u32>()?)),
        t if t == gstreamer::DateTime::static_type() => Ok(format!(
            "DateTime: {}",
            value
                .get::<gstreamer::DateTime>()?
                .ok_or(Error::msg("No DateTime"))?
        )),
        t if t == gstreamer::sample::Sample::static_type() => Ok(format!(
            "Sample: {:?}",
            value
                .get::<gstreamer::sample::Sample>()?
                .ok_or(Error::msg("No Sample"))?
        )),
        t => Ok(format!("Value of unhandled type {}: {:?}", t, value)),
    }
}
