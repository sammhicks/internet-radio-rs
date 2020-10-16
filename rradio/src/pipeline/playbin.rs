//! A wrapper around a gstreamer playbin

use anyhow::{Context, Result};
use glib::object::ObjectExt;
use gstreamer::{ElementExt, ElementExtManual};
use log::{debug, error};

pub use rradio_messages::PipelineState;

pub fn gstreamer_state_to_pipeline_state(state: gstreamer::State) -> Result<PipelineState> {
    match state {
        gstreamer::State::VoidPending => Ok(PipelineState::VoidPending),
        gstreamer::State::Null => Ok(PipelineState::Null),
        gstreamer::State::Ready => Ok(PipelineState::Ready),
        gstreamer::State::Paused => Ok(PipelineState::Paused),
        gstreamer::State::Playing => Ok(PipelineState::Playing),
        _ => Err(anyhow::Error::msg(format!("Unknown state {:?}", state))),
    }
}

#[derive(Clone)]
pub struct Playbin(gstreamer::Element);

impl Playbin {
    pub fn new() -> Result<Self> {
        let playbin_element = gstreamer::ElementFactory::make("playbin", None)
            .context("Failed to create a playbin")?;

        let flags = playbin_element
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
        playbin_element
            .set_property("flags", &flags)
            .context("Failed to set playbin flags")?;

        Ok(Self(playbin_element))
    }

    pub fn bus(&self) -> Result<gstreamer::Bus> {
        self.0.get_bus().context("Playbin has no bus")
    }

    pub fn pipeline_state(&self) -> Result<PipelineState> {
        let (success, state, _) = self.0.get_state(gstreamer::ClockTime::none());
        success.context("Unable to get state")?;
        gstreamer_state_to_pipeline_state(state)
    }

    pub fn set_pipeline_state(&self, state: PipelineState) -> Result<()> {
        let state = match state {
            PipelineState::VoidPending => gstreamer::State::VoidPending,
            PipelineState::Null => gstreamer::State::Null,
            PipelineState::Ready => gstreamer::State::Ready,
            PipelineState::Paused => gstreamer::State::Paused,
            PipelineState::Playing => gstreamer::State::Playing,
        };
        self.0.set_state(state).with_context(|| {
            format!(
                "Unable to set the playbin pipeline to the `{:?}` state",
                state
            )
        })?;
        Ok(())
    }

    pub fn play_pause(&self) -> Result<()> {
        match self.pipeline_state()? {
            PipelineState::Paused => self.set_pipeline_state(PipelineState::Playing),
            PipelineState::Playing => self.set_pipeline_state(PipelineState::Paused),
            _ => Ok(()),
        }
    }

    pub fn set_url(&self, url: &str) -> Result<()> {
        self.set_pipeline_state(PipelineState::Null)?;
        self.0
            .set_property("uri", &glib::Value::from(url))
            .with_context(|| format!("Unable to set the playbin url to `{}`", url))?;
        Ok(())
    }

    pub fn play_url(&self, url: &str) -> Result<()> {
        self.set_url(url)?;
        self.set_pipeline_state(PipelineState::Playing)?;
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
}

impl Drop for Playbin {
    fn drop(&mut self) {
        if let Err(err) = self.set_pipeline_state(PipelineState::Null) {
            error!("{:?}", err);
        }
    }
}