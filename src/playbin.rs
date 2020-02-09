use anyhow::{Context, Error, Result};
use glib::object::ObjectExt;
use gstreamer::{ElementExt, ElementExtManual, State, StateChangeError};
use log::error;

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

    pub fn get_bus(&self) -> Result<gstreamer::Bus> {
        self.0.get_bus().ok_or(Error::msg("playbin has no bus"))
    }

    pub fn get_state(&self) -> Result<State, StateChangeError> {
        let (success, state, _) = self.0.get_state(gstreamer::ClockTime::none());
        success.map(|_| state)
    }

    pub fn set_state(&self, state: State) -> Result<()> {
        self.0.set_state(state).context(format!(
            "Unable to set the playbin to the `{:?}` state",
            state
        ))?;
        Ok(())
    }

    pub fn set_url(&self, url: &str) -> Result<()> {
        self.set_state(State::Null)?;
        self.0
            .set_property("uri", &glib::Value::from(url))
            .context(format!("Unable to set the playbin url to `{}`", url))
    }
}

impl Drop for Playbin {
    fn drop(&mut self) {
        if let Err(err) = self.set_state(State::Null) {
            error!("{}", err);
        }
    }
}
