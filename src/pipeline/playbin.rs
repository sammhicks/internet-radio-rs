use anyhow::{Context, Error, Result};
use glib::object::ObjectExt;
use gstreamer::{ElementExt, ElementExtManual, State};
use log::{debug, error};

#[derive(Clone)]
pub struct Playbin(gstreamer::Element);

impl Playbin {
    pub fn new() -> Result<Self> {
        let playbin = gstreamer::ElementFactory::make("playbin", None)?;

        let flags = playbin.get_property("flags")?;
        let flags_class = glib::FlagsClass::new(flags.type_())
            .ok_or_else(|| Error::msg("Failed to create a flags class"))?;
        let flags = flags_class
            .builder_with_value(flags)
            .unwrap()
            .unset_by_nick("text")
            .unset_by_nick("video")
            .build()
            .ok_or_else(|| Error::msg("Failed to set flags"))?;
        playbin.set_property("flags", &flags)?;

        Ok(Self(playbin))
    }

    pub fn get_bus(&self) -> Result<gstreamer::Bus> {
        self.0
            .get_bus()
            .ok_or_else(|| Error::msg("playbin has no bus"))
    }

    pub fn get_state(&self) -> Result<State> {
        let (success, state, _) = self.0.get_state(gstreamer::ClockTime::none());
        success.context("Unable to get state")?;
        Ok(state)
    }

    pub fn set_state(&self, state: State) -> Result<()> {
        self.0.set_state(state).context(format!(
            "Unable to set the playbin to the `{:?}` state",
            state
        ))?;
        Ok(())
    }

    pub fn play_pause(&self) -> Result<()> {
        match self.get_state()? {
            State::Paused => self.set_state(State::Playing),
            State::Playing => self.set_state(State::Paused),
            _ => Ok(()),
        }
    }

    pub fn set_url(&self, url: &str) -> Result<()> {
        self.set_state(State::Null)?;
        self.0
            .set_property("uri", &glib::Value::from(url))
            .context(format!("Unable to set the playbin url to `{}`", url))?;
        self.set_state(State::Playing)?;
        Ok(())
    }

    pub fn is_src_of(&self, message: gstreamer_sys::GstMessage) -> bool {
        use glib::translate::ToGlibPtr;
        use gstreamer_sys::GstElement;
        let playbin_ptr: *const GstElement = self.0.to_glib_none().0;
        let message_src_ptr = message.src as *const GstElement;
        playbin_ptr == message_src_ptr
    }

    pub fn change_volume(&self, offset: i32) -> Result<()> {
        #[allow(clippy::cast_possible_truncation)]
        let current_volume = (100.0 * self.0.get_property("volume")?.get_some::<f64>()?) as i32;

        debug!("Current Volume: {}", current_volume);

        let new_volume = (current_volume + offset).max(0).min(1000);

        debug!("New Volume: {}", new_volume);

        self.0
            .set_property("volume", &(f64::from(new_volume) / 100.0))?;

        Ok(())
    }
}

impl Drop for Playbin {
    fn drop(&mut self) {
        if let Err(err) = self.set_state(State::Null) {
            error!("{:?}", err);
        }
    }
}
