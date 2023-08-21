//! A wrapper around a gstreamer playbin

use std::convert::TryInto;
use std::time::Duration;

use glib::{object::ObjectExt, Cast};
use gstreamer::prelude::{ElementExt, ElementExtManual};
use gstreamer_audio::prelude::StreamVolumeExt;

pub use rradio_messages::{PipelineError, PipelineState};

pub fn gstreamer_state_to_pipeline_state(state: gstreamer::State) -> PipelineState {
    match state {
        gstreamer::State::VoidPending => PipelineState::VoidPending,
        gstreamer::State::Null => PipelineState::Null,
        gstreamer::State::Ready => PipelineState::Ready,
        gstreamer::State::Paused => PipelineState::Paused,
        gstreamer::State::Playing => PipelineState::Playing,
    }
}

pub struct Playbin(gstreamer::Element);

impl Playbin {
    pub fn new(config: &crate::config::Config) -> Result<Self, anyhow::Error> {
        use anyhow::Context;

        let playbin_element = gstreamer::ElementFactory::make("playbin")
            .build()
            .context("Failed to create a playbin")?;

        let flags: glib::Value = playbin_element.property("flags");
        let flags_class =
            glib::FlagsClass::new(flags.type_()).context("Failed to create a flags class")?;
        let flags = flags_class
            .builder_with_value(flags)
            .unwrap()
            .unset_by_nick("text")
            .unset_by_nick("video")
            .build()
            .context("Failed to set flags")?;
        playbin_element.set_property_from_value("flags", &flags);

        if let Some(buffering_duration) = config.buffering_duration {
            let duration_nanos: i64 = buffering_duration
                .as_nanos()
                .try_into()
                .context("Bad buffer duration")?;

            playbin_element.set_property("buffer-duration", duration_nanos);
        }

        let playbin = Self(playbin_element);

        playbin.set_volume(config.initial_volume)?;

        Ok(playbin)
    }

    pub fn bus(&self) -> Result<gstreamer::Bus, anyhow::Error> {
        use anyhow::Context;
        self.0.bus().context("Playbin has no bus")
    }

    pub fn pipeline_state(&self) -> Result<PipelineState, PipelineError> {
        let (success, state, _) = self.0.state(gstreamer::ClockTime::default());
        if success.is_ok() {
            Ok(gstreamer_state_to_pipeline_state(state))
        } else {
            Err(rradio_messages::PipelineError("Failed to get state".into()))
        }
    }

    pub fn set_pipeline_state(&self, state: PipelineState) -> Result<(), PipelineError> {
        let gstreamer_state = match state {
            PipelineState::VoidPending => gstreamer::State::VoidPending,
            PipelineState::Null => gstreamer::State::Null,
            PipelineState::Ready => gstreamer::State::Ready,
            PipelineState::Paused => gstreamer::State::Paused,
            PipelineState::Playing => gstreamer::State::Playing,
        };
        self.0.set_state(gstreamer_state).map_err(|_err| {
            rradio_messages::PipelineError(format!("Failed to set state to {state}").into())
        })?;
        Ok(())
    }

    pub fn play_pause(&self) -> Result<(), PipelineError> {
        match self.pipeline_state()? {
            PipelineState::Paused => {
                self.set_pipeline_state(PipelineState::Playing)?;
                self.set_is_muted(false)?;
            }
            PipelineState::Playing => {
                if self.duration().is_some() {
                    self.set_pipeline_state(PipelineState::Paused)?;
                } else {
                    self.toggle_is_muted()?;
                }
            }
            _ => (),
        }

        Ok(())
    }

    pub fn set_url(&self, url: &str) -> Result<(), PipelineError> {
        self.set_pipeline_state(PipelineState::Null)?;
        self.0.set_property("uri", url);
        Ok(())
    }

    pub fn play_url(&self, url: &str) -> Result<(), PipelineError> {
        self.set_url(url)?;
        self.set_pipeline_state(PipelineState::Playing)
    }

    pub fn is_src_of(&self, message: gstreamer_sys::GstMessage) -> bool {
        use glib::translate::ToGlibPtr;
        use gstreamer_sys::GstElement;
        let playbin_ptr: *const GstElement = self.0.to_glib_none().0;
        let message_src_ptr = message.src as *const GstElement;
        playbin_ptr == message_src_ptr
    }

    fn stream_volume(&self) -> Result<&gstreamer_audio::StreamVolume, PipelineError> {
        self.0
            .dynamic_cast_ref::<gstreamer_audio::StreamVolume>()
            .ok_or_else(|| rradio_messages::PipelineError("Playbin has no volume".into()))
    }

    pub fn is_muted(&self) -> Result<bool, PipelineError> {
        Ok(self.stream_volume()?.is_muted())
    }

    pub fn set_is_muted(&self, is_muted: bool) -> Result<(), PipelineError> {
        tracing::debug!(is_muted, "Setting mute");

        self.stream_volume()?.set_mute(is_muted);

        Ok(())
    }

    pub fn toggle_is_muted(&self) -> Result<bool, PipelineError> {
        let stream_volume = self.stream_volume()?;

        let is_muted = !stream_volume.is_muted();

        tracing::debug!(is_muted, "Setting mute");

        stream_volume.set_mute(is_muted);

        Ok(is_muted)
    }

    pub fn volume(&self) -> Result<i32, PipelineError> {
        let current_volume = self
            .stream_volume()?
            .volume(gstreamer_audio::StreamVolumeFormat::Db);

        let scaled_volume = unsafe { current_volume.round().to_int_unchecked::<i32>() }
            + rradio_messages::VOLUME_ZERO_DB;

        tracing::debug!("Current Volume: {}", scaled_volume);

        Ok(scaled_volume)
    }

    pub fn set_volume(&self, volume: i32) -> Result<i32, PipelineError> {
        let volume = volume.clamp(rradio_messages::VOLUME_MIN, rradio_messages::VOLUME_MAX);
        tracing::debug!("New Volume: {}", volume);

        self.stream_volume()?.set_volume(
            gstreamer_audio::StreamVolumeFormat::Db,
            f64::from(volume - rradio_messages::VOLUME_ZERO_DB),
        );

        Ok(volume)
    }

    pub fn position(&self) -> Option<Duration> {
        self.0
            .query_position::<gstreamer::ClockTime>()
            .map(gstreamer::ClockTime::nseconds)
            .map(Duration::from_nanos)
    }

    pub fn seek_to(&self, position: Duration) -> Result<(), PipelineError> {
        use gstreamer::SeekFlags;

        self.0
            .seek_simple(
                SeekFlags::FLUSH | SeekFlags::KEY_UNIT | SeekFlags::SNAP_NEAREST,
                gstreamer::ClockTime::from_nseconds(
                    position
                        .as_nanos()
                        .try_into()
                        .map_err(|err| tracing::error!("Cannot cast time: {}", err))
                        .unwrap_or_default(),
                ),
            )
            .map_err(|err| {
                PipelineError(
                    format!(
                        "Failed to seek to {:.2}: {}",
                        position.as_secs_f32(),
                        err.message
                    )
                    .into(),
                )
            })
    }

    pub fn duration(&self) -> Option<Duration> {
        self.0
            .query_duration::<gstreamer::ClockTime>()
            .map(gstreamer::ClockTime::nseconds)
            .map(Duration::from_nanos)
    }

    fn do_debug_pipeline(&self) -> anyhow::Result<()> {
        use anyhow::Context;

        let gst_debug_dump_dot_dir = std::env::var("GST_DEBUG_DUMP_DOT_DIR")
            .context("Failed to get GST_DEBUG_DUMP_DOT_DIR")?;

        let bin = self
            .0
            .downcast_ref::<gstreamer::Bin>()
            .context("Playbin is not a bin")?;

        gstreamer::prelude::GstBinExtManual::debug_to_dot_file_with_ts(
            bin,
            gstreamer::DebugGraphDetails::all(),
            env!("CARGO_PKG_NAME"),
        );

        tracing::info!("Created dotfile in {}", gst_debug_dump_dot_dir);

        Ok(())
    }

    pub fn debug_pipeline(&self) {
        if let Err(err) = self.do_debug_pipeline() {
            tracing::error!("{:#}", err);
        }
    }
}

impl Drop for Playbin {
    fn drop(&mut self) {
        if let Err(err) = self.set_pipeline_state(PipelineState::Null) {
            tracing::error!("{:#}", err);
        }
    }
}
