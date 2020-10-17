use anyhow::{Context, Result};
use glib::object::ObjectExt;
use gstreamer::{ElementExt, ElementExtManual, State};
use log::{debug, error};

use crate::message::PlayerState;

#[derive(Clone)]
pub struct Playbin(gstreamer::Element);

impl Playbin {
    pub fn new() -> Result<Self> {
        let playbin = gstreamer::ElementFactory::make("playbin", None)
            .context("Failed to create a playbin")?;

        let flags = playbin
            .get_property("flags")
            .context("Failed to get the playbin flags")?;
        let flags_class =
            glib::FlagsClass::new(flags.type_()).context("Failed to create a flags class")?;
        let flags = flags_class
            .builder_with_value(flags)
            .unwrap()
            .unset_by_nick("text")
            .unset_by_nick("video")
            .build()
            .context("Failed to set flags")?;
        playbin
            .set_property("flags", &flags)
            .context("Failed to set playbin flags")?;

        Ok(Self(playbin))
    }

    pub fn bus(&self) -> Result<gstreamer::Bus> {
        self.0.get_bus().context("Playbin has no bus")
    }

    pub fn pipeline_state(&self) -> Result<State> {
        let (success, state, _) = self.0.get_state(gstreamer::ClockTime::none());
        success.context("Unable to get state")?;
        Ok(state)
    }

    pub fn set_pipeline_state(&self, state: State) -> Result<()> {
        self.0.set_state(state).with_context(|| {
            format!(
                "Unable to set the playbin pipeline to the `{:?}` state",
                state
            )
        })?;
        Ok(())
    }

    pub fn play_pause(&self) -> Result<()> {
        let duration = self
            .0
            .query_duration::<gstreamer::format::Time>()
            .and_then(|t| t.nanoseconds());
        let position = self
            .0
            .query_position::<gstreamer::format::Time>()
            .and_then(|t| t.nanoseconds());

        println!(
            "{:?}",
            position.and_then(
                move |position| duration.map(|duration| (position as f64) / (duration as f64))
            )
        );

        match self.pipeline_state()? {
            State::Paused => self.set_pipeline_state(State::Playing),
            State::Playing => self.set_pipeline_state(State::Paused),
            _ => Ok(()),
        }
    }

    pub fn set_url(&self, url: &str) -> Result<()> {
        self.set_pipeline_state(State::Null)?;
        self.0
            .set_property("uri", &glib::Value::from(url))
            .with_context(|| format!("Unable to set the playbin url to `{}`", url))?;

        let buffer_duration_ns: i64 = 20_000_000_000;
        self.0
            .set_property("buffer-duration", &buffer_duration_ns)
            .with_context(|| {
                format!(
                    "Unable to set the buffer duration to {}ns",
                    buffer_duration_ns
                )
            })?;

        self.set_pipeline_state(State::Playing)?;
        Ok(())
    }

    pub fn is_src_of(&self, message: gstreamer_sys::GstMessage) -> bool {
        use glib::translate::ToGlibPtr;
        use gstreamer_sys::GstElement;
        let playbin_ptr: *const GstElement = self.0.to_glib_none().0;
        let message_src_ptr = message.src as *const GstElement;
        playbin_ptr == message_src_ptr
    }

    pub fn volume(&self) -> Result<i32> {
        #[allow(clippy::cast_possible_truncation)]
        let current_volume = (100.0 * self.0.get_property("volume")?.get_some::<f64>()?) as i32;

        debug!("Current Volume: {}", current_volume);

        Ok(current_volume)
    }

    pub fn set_volume(&self, volume: i32) -> Result<i32> {
        debug!("New Volume: {}", volume);

        self.0
            .set_property("volume", &(f64::from(volume) / 100.0))?;

        Ok(volume)
    }

    pub fn change_volume(&self, offset: i32) -> Result<i32> {
        let new_volume = (self.volume()? + offset).max(0).min(1000);

        self.set_volume(new_volume)
    }

    pub fn state(&self) -> PlayerState {
        use std::sync::Arc;
        let pipeline_state = self
            .pipeline_state()
            .unwrap_or(gstreamer::State::Null)
            .into();

        let current_track = Arc::new(None);

        let volume = self.volume().unwrap_or_default();

        PlayerState {
            pipeline_state,
            current_track,
            volume,
            buffering: 0,
        }
    }
}

impl Drop for Playbin {
    fn drop(&mut self) {
        if let Err(err) = self.set_pipeline_state(State::Null) {
            error!("{:?}", err);
        }
    }
}
