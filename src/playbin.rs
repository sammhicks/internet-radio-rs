use anyhow::{Error, Result};
use glib::object::ObjectExt;
use gstreamer::ElementExtManual;

pub struct Playbin(gstreamer::Element);

impl Playbin {
    pub fn new() -> Result<Self> {
        let playbin = gstreamer::ElementFactory::make("playbin", None)?;

        let flags = playbin.get_property("flags")?;
        let flags_class = glib::FlagsClass::new(flags.type_())
            .ok_or(Error::msg("Failed to create a flags class"))?;
        let flags = flags_class
            .builder_with_value(flags)
            .unwrap()
            .unset_by_nick("text")
            .unset_by_nick("video")
            .build()
            .ok_or(Error::msg("Failed to set flags"))?;
        playbin.set_property("flags", &flags)?;

        Ok(Playbin(playbin))
    }
}

impl Drop for Playbin {
    fn drop(&mut self) {
        if let Err(_) = self.0.set_state(gstreamer::State::Null) {
            println!("Unable to set the pipeline to the `Null` state");
        }
    }
}

impl std::ops::Deref for Playbin {
    type Target = gstreamer::Element;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
