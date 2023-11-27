use std::{collections::BTreeMap, convert::TryInto, sync::Arc, time::Duration};
use tokio::sync::{mpsc, watch};

use rradio_messages::{
    ArcStr, Command, CurrentStation, LatestError, PingTimes, StationIndex, TrackTags,
};

use super::playbin::{IgnorePipelineError, PipelineError, PipelineState, Playbin};
use crate::{
    config::Config,
    ports::PartialPortChannels,
    station::{PlaylistMetadata, Station, Track},
    stream_select::StreamSelect,
    tag::Tag,
};

enum Error {
    Station(rradio_messages::StationError),
    Pipeline,
}

impl From<rradio_messages::StationError> for Error {
    fn from(error: rradio_messages::StationError) -> Self {
        Self::Station(error)
    }
}

impl From<PipelineError> for Error {
    fn from(PipelineError: PipelineError) -> Self {
        Self::Pipeline
    }
}

struct NoPlaylist;

impl From<NoPlaylist> for PipelineError {
    fn from(NoPlaylist: NoPlaylist) -> Self {
        tracing::error!("No Playlist");

        Self
    }
}

struct PlaylistState {
    pause_before_playing: Option<std::time::Duration>,
    tracks: Arc<[Track]>,
    current_track_index: usize,
    playlist_metadata: crate::station::PlaylistMetadata,
    _playlist_handle: crate::station::PlaylistHandle,
}

impl PlaylistState {
    fn current_track(&self) -> Result<&Track, PipelineError> {
        self.tracks.get(self.current_track_index).ok_or_else(|| {
            tracing::error!(self.current_track_index, "Invalid Track Index");
            PipelineError
        })
    }

    fn goto_previous_track(&mut self) {
        self.current_track_index = if self.current_track_index == 0 {
            self.tracks.len() - 1
        } else {
            self.current_track_index - 1
        };
    }

    fn goto_next_track(&mut self) {
        self.current_track_index += 1;
        if self.current_track_index == self.tracks.len() {
            self.current_track_index = 0;
        }
    }

    fn goto_nth_track(&mut self, index: usize) {
        if index < self.tracks.len() {
            self.current_track_index = index;
        } else {
            tracing::error!(%index, length = self.tracks.len(), "Cannot change track");
        }
    }
}

#[derive(Clone, Debug)]
pub struct PlayerState {
    pub pipeline_state: PipelineState,
    pub current_station: Arc<rradio_messages::CurrentStation>,
    pub pause_before_playing: Option<Duration>,
    pub current_track_index: usize,
    pub current_track_tags: Arc<Option<TrackTags>>,
    pub is_muted: bool,
    pub volume: i32,
    pub buffering: u8,
    pub track_duration: Option<Duration>,
    pub track_position: Option<Duration>,
    pub ping_times: PingTimes,
    pub latest_error: Arc<Option<LatestError>>,
}

#[derive(Debug, Clone)]
struct StationResumeInfo {
    track_index: usize,
    track_position: Duration,
    metadata: PlaylistMetadata,
}

struct Controller {
    config: Config,
    playbin: Playbin,
    current_playlist: Option<PlaylistState>,
    published_state: PlayerState,
    station_resume_info: BTreeMap<StationIndex, StationResumeInfo>,
    new_state_tx: watch::Sender<PlayerState>,
    queued_seek: Option<Duration>,
    #[cfg(feature = "ping")]
    ping_requests_tx: tokio::sync::mpsc::UnboundedSender<Option<ArcStr>>,
}

impl Controller {
    #[cfg(feature = "ping")]
    fn clear_ping(&mut self) {
        tracing::debug!("Clearing ping");
        if self.ping_requests_tx.send(None).is_err() {
            tracing::error!("Failed to clear ping requests");
        }
        self.handle_ping_times(PingTimes::None);
    }

    #[cfg(feature = "ping")]
    fn request_ping(&mut self, url: ArcStr) {
        if self.ping_requests_tx.send(Some(url)).is_err() {
            tracing::error!("Failed to set ping request");
        }
    }

    fn play_pause(&mut self) -> Result<(), PipelineError> {
        if self.current_playlist.is_some() {
            match self.playbin.pipeline_state()? {
                PipelineState::Null | PipelineState::Ready | PipelineState::Paused => {
                    tracing::debug!("Playing pipeline");
                    self.playbin.set_pipeline_state(PipelineState::Playing)?;
                    self.playbin.set_is_muted(false)?;
                }
                PipelineState::Playing => {
                    self.playbin
                        .set_pipeline_state(if self.playbin.duration().is_some() {
                            tracing::debug!("Pausing pipeline");
                            PipelineState::Paused
                        } else {
                            tracing::debug!("Stopping pipeline");
                            PipelineState::Null
                        })?;
                }
            }

            Ok(())
        } else {
            tracing::debug!("no current playlist, ignoring play/pause");
            Ok(())
        }
    }

    #[tracing::instrument(skip(self))]
    async fn play_current_track(&mut self) -> Result<(), PipelineError> {
        #[cfg(feature = "ping")]
        self.clear_ping();

        let current_playlist = self.current_playlist.as_ref().ok_or(NoPlaylist)?;

        let track = current_playlist.current_track()?;
        let pause_before_playing = current_playlist.pause_before_playing;

        #[cfg(feature = "ping")]
        let track_url = track.url.clone();

        tracing::debug!(?track, "Playing track");

        self.playbin.set_url(&track.url)?;
        self.published_state.current_track_index = current_playlist.current_track_index;
        self.published_state.current_track_tags = Arc::new(None);
        if let Some(pause_duration) = pause_before_playing {
            tracing::info!("Pausing for {}s", pause_duration.as_secs());
            self.playbin.set_pipeline_state(PipelineState::Paused)?;
            self.broadcast_state_change();
            tokio::time::sleep(pause_duration).await;
        }
        self.playbin.set_pipeline_state(PipelineState::Playing)?;
        self.broadcast_state_change();

        #[cfg(feature = "ping")]
        self.request_ping(track_url);

        Ok(())
    }

    async fn smart_goto_previous_track(&mut self) -> Result<(), PipelineError> {
        if let Some(track_position) = self.published_state.track_position {
            if track_position < self.config.smart_goto_previous_track_duration {
                self.goto_previous_track().await
            } else {
                self.seek_to(Duration::ZERO)
            }
        } else {
            Ok(())
        }
    }

    #[tracing::instrument(skip(self))]
    async fn goto_previous_track(&mut self) -> Result<(), PipelineError> {
        self.current_playlist
            .as_mut()
            .ok_or(NoPlaylist)?
            .goto_previous_track();
        self.play_current_track().await
    }

    #[tracing::instrument(skip(self))]
    async fn goto_next_track(&mut self) -> Result<(), PipelineError> {
        self.current_playlist
            .as_mut()
            .ok_or(NoPlaylist)?
            .goto_next_track();
        self.play_current_track().await
    }

    #[tracing::instrument(skip(self))]
    async fn goto_nth_track(&mut self, index: usize) -> Result<(), PipelineError> {
        self.current_playlist
            .as_mut()
            .ok_or(NoPlaylist)?
            .goto_nth_track(index);
        self.play_current_track().await
    }

    fn seek_to(&mut self, position: Duration) -> Result<(), PipelineError> {
        self.playbin.seek_to(position)
    }

    fn clear_playlist(&mut self) {
        #[cfg(feature = "ping")]
        self.clear_ping();

        self.current_playlist = None;
        self.published_state.current_station = Arc::new(CurrentStation::NoStation);
        self.published_state.pause_before_playing = None;
        self.published_state.current_track_index = 0;
        self.published_state.current_track_tags = Arc::new(None);

        self.set_is_muted(false).ok();

        self.broadcast_state_change();

        self.playbin.set_pipeline_state(PipelineState::Null).ok();
    }

    fn play_error(&mut self, error: Error) {
        self.clear_playlist();

        match error {
            Error::Station(error) => {
                self.published_state.current_station =
                    Arc::new(CurrentStation::FailedToPlayStation { error });
            }
            Error::Pipeline => (),
        }

        self.broadcast_state_change();

        if let Some(url) = &self.config.notifications.error {
            self.playbin.play_url(url.as_str()).ignore_pipeline_error();
        }
    }

    fn broadcast_error(&mut self, error: impl AsRef<str>) {
        self.published_state.latest_error = Arc::new(Some(rradio_messages::LatestError {
            timestamp: chrono::Utc::now(),
            error: error.as_ref().into(),
        }));

        self.broadcast_state_change();
    }

    fn broadcast_state_change(&mut self) {
        self.published_state.track_duration = self.playbin.duration();
        self.published_state.track_position = self.playbin.position();
        self.published_state.is_muted = self.playbin.is_muted();

        self.new_state_tx.send(self.published_state.clone()).ok();
    }

    fn create_resume_info(
        &self,
        new_station_index: &StationIndex,
    ) -> Option<(StationIndex, StationResumeInfo)> {
        let CurrentStation::PlayingStation {
            index: Some(current_station_index),
            source_type: current_station_source_type,
            ..
        } = self.published_state.current_station.as_ref()
        else {
            return None;
        };

        if new_station_index == current_station_index {
            return None;
        }

        match current_station_source_type {
            rradio_messages::StationType::UrlList => return None,
            rradio_messages::StationType::UPnP
            | rradio_messages::StationType::CD
            | rradio_messages::StationType::Usb => (),
        }

        let station_resume_info = StationResumeInfo {
            track_index: self.published_state.current_track_index,
            track_position: self.published_state.track_position?,
            metadata: self.current_playlist.as_ref()?.playlist_metadata.clone(),
        };

        Some((current_station_index.clone(), station_resume_info))
    }

    #[tracing::instrument(skip(self))]
    fn save_resume_info(&mut self, new_station_index: &StationIndex) {
        let Some((current_station_index, station_resume_info)) =
            self.create_resume_info(new_station_index)
        else {
            return;
        };

        tracing::debug!(
            "Saving state for {}: {:?}",
            current_station_index,
            station_resume_info
        );

        self.station_resume_info
            .insert(current_station_index, station_resume_info);
    }

    #[tracing::instrument(skip(self))]
    async fn play_station(&mut self, new_station: Station) -> Result<(), Error> {
        if let Some(index) = new_station.index() {
            self.save_resume_info(index);
        }

        let resume_info = new_station
            .index()
            .and_then(|index| self.station_resume_info.remove(index));

        self.clear_playlist();

        self.published_state.current_station =
            Arc::new(rradio_messages::CurrentStation::PlayingStation {
                index: new_station.index().cloned(),
                title: new_station.title().map(ArcStr::from),
                source_type: new_station.station_type(),
                tracks: None,
            });

        self.set_is_muted(false).ok();

        self.broadcast_state_change();

        let playlist = new_station
            .into_playlist(
                resume_info
                    .as_ref()
                    .map(|resume_info| &resume_info.metadata),
            )
            .await?;

        tracing::debug!("Station tracks: {:?}", playlist.tracks);

        let playlist_tracks = if playlist.tracks.len() > 1 {
            let prefix_notification = self
                .config
                .notifications
                .playlist_prefix
                .clone()
                .into_iter()
                .map(Track::notification);

            let suffix_notification = self
                .config
                .notifications
                .playlist_suffix
                .clone()
                .into_iter()
                .map(Track::notification);

            prefix_notification
                .chain(playlist.tracks)
                .chain(suffix_notification)
                .collect()
        } else {
            Arc::<[Track]>::from(playlist.tracks)
        };

        tracing::trace!(
            "Resume Info for {:?}: {:?}",
            playlist.station_index,
            resume_info
        );

        self.current_playlist = Some(PlaylistState {
            pause_before_playing: None,
            tracks: playlist_tracks.clone(),
            current_track_index: resume_info
                .as_ref()
                .map_or(0, |resume_info| resume_info.track_index),
            playlist_metadata: playlist.metadata,
            _playlist_handle: playlist.handle,
        });

        self.published_state.current_station =
            Arc::new(rradio_messages::CurrentStation::PlayingStation {
                index: playlist.station_index,
                title: playlist.station_title.map(ArcStr::from),
                source_type: playlist.station_type,
                tracks: Some(playlist_tracks),
            });

        self.published_state.pause_before_playing = None;

        self.queued_seek = resume_info.map(|resume_info| resume_info.track_position);

        self.play_current_track().await?;

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    fn set_is_muted(&mut self, is_muted: bool) -> Result<(), PipelineError> {
        self.playbin.set_is_muted(is_muted)?;
        self.published_state.is_muted = is_muted;
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    fn set_volume(&mut self, volume: i32) -> Result<(), PipelineError> {
        self.published_state.volume = self.playbin.set_volume(volume)?;
        self.broadcast_state_change();
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    fn change_volume(&mut self, direction: i32) -> Result<(), PipelineError> {
        // First round the current volume to the nearest multiple of the volume offset
        let current_volume = f64::from(self.playbin.volume()?);
        let volume_offset = f64::from(self.config.volume_offset);

        let rounded_volume = volume_offset * (current_volume / volume_offset).round();
        let rounded_volume = unsafe { rounded_volume.round().to_int_unchecked::<i32>() };

        // Then set the volume to the next increment
        self.set_volume(rounded_volume + direction * self.config.volume_offset)
    }

    #[tracing::instrument(skip(self))]
    async fn handle_command(&mut self, command: Command) -> Result<(), Error> {
        tracing::debug!("Processing Command");
        match command {
            Command::SetChannel(index) => {
                self.play_station(Station::load(&self.config, index)?)
                    .await?;
                Ok(())
            }
            Command::PlayPause => self.play_pause(),
            Command::SmartPreviousItem => self.smart_goto_previous_track().await,
            Command::PreviousItem => self.goto_previous_track().await,
            Command::NextItem => self.goto_next_track().await,
            Command::NthItem(index) => self.goto_nth_track(index).await,
            Command::SeekTo(position) => self.seek_to(position),
            Command::SeekBackwards(offset) => self.playbin.position().map_or(Ok(()), |position| {
                self.seek_to(position.saturating_sub(offset))
            }),
            Command::SeekForwards(offset) => self.playbin.position().map_or(Ok(()), |position| {
                self.seek_to(position.saturating_add(offset))
            }),
            Command::SetIsMuted(is_muted) => {
                self.set_is_muted(is_muted)?;
                self.broadcast_state_change();
                Ok(())
            }
            Command::ToggleIsMuted => {
                self.published_state.is_muted = self.playbin.toggle_is_muted()?;
                self.broadcast_state_change();
                Ok(())
            }
            Command::VolumeUp => self.change_volume(1),
            Command::VolumeDown => self.change_volume(-1),
            Command::SetVolume(volume) => self.set_volume(volume),
            Command::SetPlaylist { title, tracks } => {
                self.play_station(Station::UrlList {
                    index: None,
                    title: Some(title),
                    tracks: tracks.into_iter().map(Track::from).collect(),
                })
                .await?;
                Ok(())
            }
            Command::Eject => {
                if let CurrentStation::PlayingStation {
                    source_type: rradio_messages::StationType::CD,
                    ..
                } = self.published_state.current_station.as_ref()
                {
                    self.clear_playlist();
                }

                #[cfg(feature = "cd")]
                {
                    self.station_resume_info
                        .remove(self.config.cd_config.station.as_str());

                    if let Err(err) =
                        crate::station::eject_cd(self.config.cd_config.device.as_str()).await
                    {
                        self.broadcast_error(format!("{err}"));
                    }

                    Ok(())
                }

                #[cfg(not(feature = "cd"))]
                {
                    tracing::warn!("Ignoring Eject");

                    Ok(())
                }
            }
            Command::DebugPipeline => {
                self.playbin.debug_pipeline();
                Ok(())
            }
        }
        .map_err(Error::from)
    }

    #[tracing::instrument(skip(self, message, gstreamer_messages))]
    #[allow(clippy::too_many_lines)]
    async fn handle_gstreamer_message(
        &mut self,
        message: &gstreamer::Message,
        gstreamer_messages: &async_channel::Receiver<gstreamer::Message>,
    ) -> Result<(), PipelineError> {
        use gstreamer::MessageView;

        match message.view() {
            MessageView::Buffering(buffering) => {
                tracing::trace!(
                    parent: &tracing::trace_span!("buffering"),
                    "{}",
                    buffering.percent()
                );

                match buffering.percent().try_into() {
                    Ok(buffering) => {
                        self.published_state.buffering = buffering;
                        self.broadcast_state_change();
                    }
                    Err(_err) => {
                        tracing::warn!("Bad buffering value: {}", buffering.percent());
                    }
                }

                Ok(())
            }
            MessageView::Tag(tag) => {
                let mut new_tags = self
                    .published_state
                    .current_track_tags
                    .as_ref()
                    .clone()
                    .unwrap_or_default();

                for (i, (name, value)) in tag.tags().as_ref().iter().enumerate() {
                    let tag = Tag::from_value(name, &value);
                    tracing::trace!(parent: &tracing::trace_span!("tag"), "{} - {:?}", i, tag);

                    match tag {
                        Ok(Tag::Title(title)) => new_tags.title = Some(title),
                        Ok(Tag::Organisation(organisation)) => {
                            new_tags.organisation = Some(organisation);
                        }
                        Ok(Tag::Artist(artist)) => new_tags.artist = Some(artist),
                        Ok(Tag::Album(album)) => new_tags.album = Some(album),
                        Ok(Tag::Genre(genre)) => new_tags.genre = Some(genre),
                        Ok(Tag::Image(image)) => new_tags.image = Some(image),
                        Ok(Tag::Comment(comment)) => new_tags.comment = Some(comment),
                        Ok(Tag::Unknown { .. }) => (),
                        Err(err) => tracing::warn!("Failed to decode tag: {err}"),
                    }
                }

                if let Some(playlist_state) = &self.current_playlist {
                    if let Ok(track) = playlist_state.current_track() {
                        if !track.is_notification && new_tags != TrackTags::default() {
                            self.published_state.current_track_tags = Arc::new(Some(new_tags));
                            self.broadcast_state_change();
                        }
                    }
                }

                Ok(())
            }
            MessageView::StateChanged(state_changed) => {
                if self.playbin.is_src_of(state_changed) {
                    let new_state = state_changed.current();

                    self.published_state.pipeline_state =
                        super::playbin::gstreamer_state_to_pipeline_state(new_state)?;

                    self.broadcast_state_change();

                    tracing::debug!(
                        parent: &tracing::debug_span!("state_change"),
                        "{:?}",
                        new_state
                    );

                    if let gstreamer::State::Playing = new_state {
                        if let Some(position) = self.queued_seek.take() {
                            self.seek_to(position)?;
                        }
                    }
                }
                Ok(())
            }
            MessageView::Eos(..) => {
                tracing::debug!(parent: &tracing::debug_span!("end_of_stream"), "");

                if let Some(current_playlist) = &self.current_playlist {
                    if self.published_state.track_duration.is_some() {
                        if current_playlist.tracks.len() > 1 {
                            self.goto_next_track().await
                        } else {
                            self.clear_playlist();
                            Ok(())
                        }
                    } else {
                        let Some(current_playlist) = self.current_playlist.as_mut() else {
                            return Ok(());
                        };

                        let pause_before_playing =
                            current_playlist.pause_before_playing.unwrap_or_default()
                                + self.config.pause_before_playing_increment;

                        current_playlist.pause_before_playing = Some(pause_before_playing);
                        self.published_state.pause_before_playing = Some(pause_before_playing);

                        if pause_before_playing > self.config.max_pause_before_playing {
                            tracing::error!(
                                ?pause_before_playing,
                                ?self.config.max_pause_before_playing,
                                "Max pause_before_playing timeout exceeded"
                            );
                            Err(PipelineError)
                        } else {
                            self.play_current_track().await
                        }
                    }
                } else {
                    Ok(self.playbin.set_pipeline_state(PipelineState::Null)?)
                }
            }
            MessageView::Error(err) => {
                fn convert_domain(glib_error: &glib::Error) -> String {
                    macro_rules! convert_domain {
                        ($($domain:ty,)*) => {
                            $(
                                if let Some(domain) = glib_error.kind::<$domain>() {
                                    return format!("{}::{:?}", stringify!($domain), domain);
                                }
                            )*
                        };
                    }

                    convert_domain!(
                        glib::ConvertError,
                        glib::FileError,
                        glib::KeyFileError,
                        glib::MarkupError,
                        gstreamer::CoreError,
                        gstreamer::LibraryError,
                        gstreamer::ParseError,
                        gstreamer::PluginError,
                        gstreamer::ResourceError,
                        gstreamer::StreamError,
                        gstreamer::URIError,
                    );

                    let domain = glib_error.domain();
                    domain.as_str().as_str().into()
                }

                let glib_error = err.error();

                let error = convert_domain(&glib_error);
                let code = unsafe { (*glib_error.as_ptr()).code };
                let error_message = glib_error.message();
                let debug_message = err.debug();

                tracing::error!(
                    ?error,
                    ?code,
                    ?error_message,
                    ?debug_message,
                    "gstreamer error"
                );

                self.broadcast_error(format!("gstreamer error: error={error:?} code={code:?} error_message={error_message:?} debug_message={debug_message:?}"));

                {
                    let (error, kind): (Box<dyn std::fmt::Debug>, &'static str) =
                        if let Some(stream_error) = glib_error.kind::<gstreamer::StreamError>() {
                            (Box::from(stream_error), "Stream Error")
                        } else if let Some(resource_error) =
                            glib_error.kind::<gstreamer::ResourceError>()
                        {
                            (Box::from(resource_error), "ResourceError")
                        } else {
                            return Err(PipelineError);
                        };

                    tracing::debug!(?error, "Caught {kind}, playing next track");
                }

                self.playbin.set_pipeline_state(PipelineState::Null)?;
                tracing::debug!("Draining message queue...");

                {
                    let _ = tracing::trace_span!("Draining message queue").enter();

                    // Drain the message queue, thus draining potential other error messages
                    while gstreamer_messages.try_recv().is_ok() {
                        tracing::trace!(message = ?message.view(), "Ignoring Message");
                    }
                }

                tracing::debug!("Finished draining message queue");

                self.goto_next_track().await?;

                Ok(())
            }
            _ => Ok(()),
        }
    }

    #[cfg(feature = "ping")]
    fn handle_ping_times(&mut self, ping_times: rradio_messages::PingTimes) {
        self.published_state.ping_times = ping_times;
        self.broadcast_state_change();
    }
}

enum Message {
    Command(Command),
    FromGStreamer(gstreamer::Message),
    #[cfg(feature = "ping")]
    PingTimes(PingTimes),
}

/// Initialise the gstreamer pipeline, and process incoming commands
#[allow(clippy::too_many_lines)]
pub fn run(
    config: Config,
) -> anyhow::Result<(
    impl std::future::Future<Output = ()>,
    PartialPortChannels<()>,
)> {
    gstreamer::init()?;
    let (playbin, bus_stream) = Playbin::new(&config)
        .map_err(|PipelineError| anyhow::anyhow!("Failed to create playbin"))?;

    if let Some(url) = &config.notifications.ready {
        playbin.play_url(url).ignore_pipeline_error();
    }

    let (commands_tx, commands_rx) = mpsc::unbounded_channel();

    let published_state = PlayerState {
        pipeline_state: playbin.pipeline_state().unwrap_or(PipelineState::Null),
        current_station: Arc::new(CurrentStation::NoStation),
        pause_before_playing: None,
        current_track_index: 0,
        current_track_tags: Arc::new(None),
        is_muted: playbin.is_muted(),
        volume: playbin.volume().unwrap_or_default(),
        buffering: 0,
        track_duration: None,
        track_position: None,
        ping_times: rradio_messages::PingTimes::None,
        latest_error: Arc::new(None),
    };

    let (new_state_tx, new_state_rx) = watch::channel(published_state.clone());

    #[cfg(feature = "ping")]
    let (ping_task, ping_requests_tx, ping_times_rx) =
        super::ping::run(config.ping_config.clone())?;

    let mut controller = Controller {
        config,
        playbin,
        current_playlist: None,
        published_state,
        station_resume_info: BTreeMap::new(),
        new_state_tx,
        queued_seek: None,
        #[cfg(feature = "ping")]
        ping_requests_tx,
    };

    let task = async move {
        use futures::StreamExt;

        #[cfg(feature = "ping")]
        let ping_handle = tokio::task::spawn(ping_task);

        let commands = futures::stream::unfold(commands_rx, |mut commands_rx| async {
            let message = Message::Command(commands_rx.recv().await?);
            Some((message, commands_rx))
        });

        let bus_side_stream = bus_stream.clone_receiver();

        let bus_stream = bus_stream.map(Message::FromGStreamer);

        #[cfg(feature = "ping")]
        let messages = {
            let ping_stream = futures::stream::unfold(ping_times_rx, |mut commands_rx| async {
                let ping_times = commands_rx.recv().await?;
                Some((Message::PingTimes(ping_times), commands_rx))
            });

            StreamSelect((commands, bus_stream, ping_stream))
        };

        #[cfg(not(feature = "ping"))]
        let messages = StreamSelect((commands, bus_stream));

        tokio::pin!(messages);

        let timeout = Duration::from_millis(1000 / 3);

        loop {
            match tokio::time::timeout(timeout, messages.next()).await {
                Ok(None) => break,
                Ok(Some(message)) => {
                    if let Err(error) = match message {
                        Message::Command(command) => controller.handle_command(command).await,
                        Message::FromGStreamer(message) => controller
                            .handle_gstreamer_message(&message, &bus_side_stream)
                            .await
                            .map_err(Error::from),
                        #[cfg(feature = "ping")]
                        Message::PingTimes(ping_times) => {
                            controller.handle_ping_times(ping_times);
                            Ok(())
                        }
                    } {
                        controller.play_error(error);
                    }
                }
                Err(_) => controller.broadcast_state_change(),
            }
        }

        #[cfg(feature = "ping")]
        {
            drop(controller.ping_requests_tx);
            if let Err(err) = ping_handle.await {
                tracing::error!("Error with ping routine: {}", err);
            }
        }
    };

    Ok((
        task,
        PartialPortChannels {
            commands_tx,
            player_state_rx: new_state_rx,
            shutdown_signal: (),
        },
    ))
}
