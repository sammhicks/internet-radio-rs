//! A wrapper around a gstreamer playbin

use std::{convert::TryInto, pin::Pin, time::Duration};

use glib::{object::ObjectExt, Cast};
use gstreamer::prelude::{ElementExt, ElementExtManual};
use gstreamer_audio::prelude::StreamVolumeExt;

pub use rradio_messages::PipelineState;

pub struct PipelineError;

pub trait IgnorePipelineError {
    fn ignore_pipeline_error(self);
}

impl IgnorePipelineError for Result<(), PipelineError> {
    fn ignore_pipeline_error(self) {
        match self {
            Ok(()) | Err(PipelineError) => (),
        }
    }
}

trait PipelineErrorContext: Sized {
    type Output;

    fn with_context<C: std::fmt::Display>(
        self,
        context: impl FnOnce() -> C,
    ) -> Result<Self::Output, PipelineError>;

    fn context<C: std::fmt::Display>(self, context: C) -> Result<Self::Output, PipelineError> {
        self.with_context(|| context)
    }
}

impl<T> PipelineErrorContext for Option<T> {
    type Output = T;

    fn with_context<C: std::fmt::Display>(
        self,
        context: impl FnOnce() -> C,
    ) -> Result<Self::Output, PipelineError> {
        self.ok_or_else(|| {
            let context = context();
            tracing::error!("{context}");
            PipelineError
        })
    }
}

impl<T, E: std::fmt::Display> PipelineErrorContext for Result<T, E> {
    type Output = T;

    fn with_context<C: std::fmt::Display>(
        self,
        context: impl FnOnce() -> C,
    ) -> Result<Self::Output, PipelineError> {
        self.map_err(|err| {
            let context = context();
            tracing::error!("{context}: {err}");
            PipelineError
        })
    }
}

impl From<gstreamer::StateChangeError> for PipelineError {
    fn from(error: gstreamer::StateChangeError) -> Self {
        tracing::error!("{error}");
        Self
    }
}

pub fn gstreamer_state_to_pipeline_state(
    state: gstreamer::State,
) -> Result<PipelineState, PipelineError> {
    Ok(match state {
        gstreamer::State::VoidPending => {
            tracing::error!("current state cannot be void");
            return Err(PipelineError);
        }
        gstreamer::State::Null => PipelineState::Null,
        gstreamer::State::Ready => PipelineState::Ready,
        gstreamer::State::Paused => PipelineState::Paused,
        gstreamer::State::Playing => PipelineState::Playing,
    })
}

pub struct Playbin(gstreamer::Element);

impl Playbin {
    #[tracing::instrument]
    pub fn new(config: &crate::config::Config) -> Result<(Self, BusStream), PipelineError> {
        let playbin_element = gstreamer::ElementFactory::make("playbin")
            .build()
            .context("Failed to create a playbin")?;

        let flags: glib::Value = playbin_element.property("flags");
        let flags_class =
            glib::FlagsClass::with_type(flags.type_()).context("Failed to create a flags class")?;
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

        let bus = playbin_element.bus().context("Playbin has no bus")?;

        let playbin = Self(playbin_element);

        playbin.set_volume(config.initial_volume)?;

        Ok((playbin, BusStream::new(bus)))
    }

    #[tracing::instrument(skip(self))]
    pub fn pipeline_state(&self) -> Result<PipelineState, PipelineError> {
        let (success, state, _) = self.0.state(gstreamer::ClockTime::default());

        success?;

        gstreamer_state_to_pipeline_state(state)
    }

    #[tracing::instrument(skip(self))]
    pub fn set_pipeline_state(&self, state: PipelineState) -> Result<(), PipelineError> {
        let gstreamer_state = match state {
            PipelineState::Null => gstreamer::State::Null,
            PipelineState::Ready => gstreamer::State::Ready,
            PipelineState::Paused => gstreamer::State::Paused,
            PipelineState::Playing => gstreamer::State::Playing,
        };
        self.0.set_state(gstreamer_state)?;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn set_url(&self, url: &str) -> Result<(), PipelineError> {
        self.set_pipeline_state(PipelineState::Null)?;
        self.0.set_property("uri", url);
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn play_url(&self, url: &str) -> Result<(), PipelineError> {
        self.set_url(url)?;
        self.set_pipeline_state(PipelineState::Playing)
    }

    pub fn is_src_of(&self, message: &gstreamer::MessageRef) -> bool {
        message
            .src()
            .is_some_and(|message_src| message_src == &self.0)
    }

    #[tracing::instrument(skip(self))]
    fn stream_volume(&self) -> Result<&gstreamer_audio::StreamVolume, PipelineError> {
        self.0
            .dynamic_cast_ref::<gstreamer_audio::StreamVolume>()
            .context("Playbin has no volume")
    }

    pub fn is_muted(&self) -> bool {
        self.stream_volume()
            .map_or(false, gstreamer_audio::prelude::StreamVolumeExt::is_muted)
    }

    #[tracing::instrument(skip(self))]
    pub fn set_is_muted(&self, is_muted: bool) -> Result<(), PipelineError> {
        tracing::debug!("Setting mute");

        self.stream_volume()?.set_mute(is_muted);

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    pub fn toggle_is_muted(&self) -> Result<bool, PipelineError> {
        let stream_volume = self.stream_volume()?;

        let is_muted = !stream_volume.is_muted();

        tracing::debug!(is_muted, "Setting mute");

        stream_volume.set_mute(is_muted);

        Ok(is_muted)
    }

    #[tracing::instrument(skip(self))]
    pub fn volume(&self) -> Result<i32, PipelineError> {
        let current_volume = self
            .stream_volume()?
            .volume(gstreamer_audio::StreamVolumeFormat::Db);

        let scaled_volume = unsafe { current_volume.round().to_int_unchecked::<i32>() }
            + rradio_messages::VOLUME_ZERO_DB;

        tracing::debug!("Current Volume: {}", scaled_volume);

        Ok(scaled_volume)
    }

    #[tracing::instrument(skip(self))]
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

    #[tracing::instrument(skip(self))]
    pub fn seek_to(&self, position: Duration) -> Result<(), PipelineError> {
        use gstreamer::SeekFlags;

        self.0
            .seek_simple(
                SeekFlags::FLUSH | SeekFlags::KEY_UNIT | SeekFlags::SNAP_NEAREST,
                gstreamer::ClockTime::from_nseconds(
                    position
                        .as_nanos()
                        .try_into()
                        .context("Failed to cast time")?,
                ),
            )
            .context("Failed to seek")
    }

    pub fn duration(&self) -> Option<Duration> {
        self.0
            .query_duration::<gstreamer::ClockTime>()
            .map(gstreamer::ClockTime::nseconds)
            .map(Duration::from_nanos)
    }

    pub fn debug_pipeline(&self) {
        let debug_pipeline = || {
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
        };

        debug_pipeline().ignore_pipeline_error();
    }
}

impl Drop for Playbin {
    fn drop(&mut self) {
        self.set_pipeline_state(PipelineState::Null)
            .ignore_pipeline_error();
    }
}

#[pin_project::pin_project(PinnedDrop)]
pub struct BusStream {
    bus: gstreamer::Bus,
    #[pin]
    receiver: async_channel::Receiver<gstreamer::Message>,
}

impl BusStream {
    pub fn new(bus: gstreamer::Bus) -> Self {
        let (sender, receiver) = async_channel::unbounded();

        bus.set_sync_handler(move |_, message| {
            let _ = sender.send_blocking(message.to_owned());

            gstreamer::BusSyncReply::Drop
        });

        Self { bus, receiver }
    }

    pub fn clone_receiver(&self) -> async_channel::Receiver<gstreamer::Message> {
        self.receiver.clone()
    }
}

#[pin_project::pinned_drop]
impl PinnedDrop for BusStream {
    fn drop(self: Pin<&mut Self>) {
        self.bus.unset_sync_handler();
    }
}

impl futures_util::Stream for BusStream {
    type Item = gstreamer::Message;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.project().receiver.poll_next(cx)
    }
}

impl futures_util::stream::FusedStream for BusStream {
    fn is_terminated(&self) -> bool {
        self.receiver.is_terminated()
    }
}
