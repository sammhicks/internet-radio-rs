use std::sync::Arc;
use std::{convert::TryInto, time::Duration};
use tokio::sync::{broadcast, mpsc, watch};

use rradio_messages::{Command, Error, LogMessage, PingTimes, PipelineError, TrackTags};

use super::playbin::{PipelineState, Playbin};
use crate::{
    config::Config,
    ports::PartialPortChannels,
    station::{Station, Track},
    tag::Tag,
};

#[derive(Clone, Debug)]
pub struct LogMessageSource(broadcast::Sender<LogMessage>);

impl LogMessageSource {
    pub fn subscribe(&self) -> broadcast::Receiver<LogMessage> {
        self.0.subscribe()
    }
}

struct PlaylistState {
    pause_before_playing: Option<std::time::Duration>,
    tracks: Arc<[Track]>,
    current_track_index: usize,
    _station_handle: crate::station::Handle,
}

impl PlaylistState {
    fn current_track(&self) -> Result<&Track, Error> {
        self.tracks
            .get(self.current_track_index)
            .ok_or(Error::InvalidTrackIndex(self.current_track_index))
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
}

#[derive(Clone, Debug)]
pub struct PlayerState {
    pub pipeline_state: PipelineState,
    pub current_station: Arc<Option<rradio_messages::Station>>,
    pub pause_before_playing: Option<Duration>,
    pub current_track_index: usize,
    pub current_track_tags: Arc<Option<TrackTags>>,
    pub volume: i32,
    pub buffering: u8,
    pub track_duration: Option<Duration>,
    pub track_position: Option<Duration>,
    pub ping_times: PingTimes,
}

struct Controller {
    config: Config,
    playbin: Playbin,
    current_playlist: Option<PlaylistState>,
    published_state: PlayerState,
    new_state_tx: watch::Sender<PlayerState>,
    log_message_tx: broadcast::Sender<LogMessage>,
    #[cfg(feature = "ping")]
    ping_requests_tx: tokio::sync::mpsc::UnboundedSender<Option<rradio_messages::ArcStr>>,
}

impl Controller {
    #[cfg(feature = "ping")]
    fn clear_ping(&mut self) {
        log::info!("Clearing ping");
        if self.ping_requests_tx.send(None).is_err() {
            log::error!("Failed to clear ping requests");
        }
        self.handle_ping_times(PingTimes::None);
    }

    #[cfg(feature = "ping")]
    fn request_ping(&mut self, url: rradio_messages::ArcStr) {
        if self.ping_requests_tx.send(Some(url)).is_err() {
            log::error!("Failed to set ping request");
        }
    }

    fn play_pause(&mut self) -> Result<(), Error> {
        if self.current_playlist.is_some() {
            self.playbin.play_pause().map_err(Error::from)
        } else {
            Ok(())
        }
    }

    async fn play_current_track(&mut self) -> Result<(), Error> {
        #[cfg(feature = "ping")]
        self.clear_ping();

        let current_playlist = self.current_playlist.as_ref().ok_or(Error::NoPlaylist)?;

        let track = current_playlist.current_track()?;
        let pause_before_playing = current_playlist.pause_before_playing;

        #[cfg(feature = "ping")]
        let track_url = track.url.clone();

        self.playbin.set_url(&track.url)?;
        self.published_state.current_track_index = current_playlist.current_track_index;
        self.published_state.current_track_tags = Arc::new(None);
        if let Some(pause_duration) = pause_before_playing {
            log::info!("Pausing for {}s", pause_duration.as_secs());
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

    async fn smart_goto_previous_track(&mut self) -> Result<(), Error> {
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

    async fn goto_previous_track(&mut self) -> Result<(), Error> {
        self.current_playlist
            .as_mut()
            .ok_or(Error::NoPlaylist)?
            .goto_previous_track();
        self.play_current_track().await
    }

    async fn goto_next_track(&mut self) -> Result<(), Error> {
        self.current_playlist
            .as_mut()
            .ok_or(Error::NoPlaylist)?
            .goto_next_track();
        self.play_current_track().await
    }

    fn seek_to(&mut self, position: Duration) -> Result<(), Error> {
        Ok(self.playbin.seek_to(position)?)
    }

    fn clear_playlist(&mut self) {
        #[cfg(feature = "ping")]
        self.clear_ping();

        self.current_playlist = None;

        self.playbin.set_pipeline_state(PipelineState::Null).ok();
    }

    fn play_error(&mut self) {
        self.clear_playlist();

        if let Some(url) = &self.config.notifications.error {
            if let Err(err) = self.playbin.play_url(&url) {
                log::error!("{:#}", err);
            }
        }
    }

    fn broadcast_state_change(&mut self) {
        self.published_state.track_duration = self.playbin.duration();
        self.published_state.track_position = self.playbin.position();

        self.new_state_tx.send(self.published_state.clone()).ok();
    }

    fn broadcast_error_message(&mut self, error: Error) {
        log::error!("{}", error);
        self.log_message_tx.send(error.into()).ok();
    }

    async fn play_station(&mut self, new_station: Station) -> Result<(), Error> {
        self.clear_playlist();

        let playlist = new_station.into_playlist()?;

        log::debug!("Station tracks: {:?}", playlist.tracks);

        let prefix_notification = self
            .config
            .notifications
            .playlist_prefix
            .iter()
            .cloned()
            .map(Track::notification);

        let suffix_notification = self
            .config
            .notifications
            .playlist_suffix
            .iter()
            .cloned()
            .map(Track::notification);

        let playlist_tracks = prefix_notification
            .chain(playlist.tracks)
            .chain(suffix_notification)
            .collect::<Arc<_>>();

        self.current_playlist = Some(PlaylistState {
            pause_before_playing: playlist.pause_before_playing,
            tracks: playlist_tracks.clone(),
            current_track_index: 0,
            _station_handle: playlist.handle,
        });

        self.published_state.current_station = Arc::new(Some(rradio_messages::Station {
            index: playlist.station_index.map(From::from),
            title: playlist.station_title.map(From::from),
            source_type: playlist.station_type,
            tracks: playlist_tracks,
        }));

        self.published_state.pause_before_playing = playlist.pause_before_playing;

        self.play_current_track().await
    }

    fn set_volume(&mut self, volume: i32) -> Result<(), Error> {
        self.published_state.volume = self.playbin.set_volume(volume)?;
        self.broadcast_state_change();
        Ok(())
    }

    fn change_volume(&mut self, direction: i32) -> Result<(), Error> {
        let current_volume = self.playbin.volume()?;

        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        let rounded_volume = self.config.volume_offset
            * ((current_volume as f32) / (self.config.volume_offset as f32)).round() as i32;

        self.set_volume(rounded_volume + direction * self.config.volume_offset)
    }

    async fn handle_command(&mut self, command: Command) -> Result<(), Error> {
        log::debug!("Command: {:?}", command);
        match command {
            Command::SetChannel(index) => {
                let new_station = Station::load(self.config.stations_directory.as_str(), index)?;
                self.play_station(new_station).await
            }
            Command::PlayPause => self.play_pause(),
            Command::SmartPreviousItem => self.smart_goto_previous_track().await,
            Command::PreviousItem => self.goto_previous_track().await,
            Command::NextItem => self.goto_next_track().await,
            Command::SeekTo(position) => self.seek_to(position),
            Command::SeekBackwards(offset) => self.playbin.position().map_or(Ok(()), |position| {
                self.seek_to(position.saturating_sub(offset))
            }),
            Command::SeekForwards(offset) => self.playbin.position().map_or(Ok(()), |position| {
                self.seek_to(position.saturating_add(offset))
            }),
            Command::VolumeUp => self.change_volume(1),
            Command::VolumeDown => self.change_volume(-1),
            Command::SetVolume(volume) => self.set_volume(volume),
            Command::PlayUrl(url) => self.play_station(Station::singleton(url.into())).await,
            Command::Eject => {
                if let Some(rradio_messages::StationType::CD) = self
                    .published_state
                    .current_station
                    .as_ref()
                    .as_ref()
                    .map(|station| station.source_type)
                {
                    self.clear_playlist();
                }

                #[cfg(feature = "cd")]
                if let Err(err) = crate::station::eject_cd(self.config.cd_config.device.as_str()) {
                    self.broadcast_error_message(err.into());
                }

                #[cfg(not(feature = "cd"))]
                log::info!("Ignoring Eject");

                Ok(())
            }
            Command::DebugPipeline => {
                self.playbin.debug_pipeline();
                Ok(())
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    async fn handle_gstreamer_message(
        &mut self,
        message: &gstreamer::Message,
    ) -> Result<(), Error> {
        use gstreamer::MessageView;
        match message.view() {
            MessageView::Buffering(b) => {
                let buffering_target = concat!(module_path!(), "::buffering");
                log::debug!(target: buffering_target, "{}", b.get_percent());

                self.published_state.buffering = b.get_percent().try_into().map_err(|_err| {
                    PipelineError(format!("Bad buffering value: {}", b.get_percent()).into())
                })?;

                self.broadcast_state_change();

                Ok(())
            }
            MessageView::Tag(tag) => {
                let mut new_tags = TrackTags::default();

                for (i, (name, value)) in tag.get_tags().as_ref().iter().enumerate() {
                    let tag = Tag::from_value(name, &value);
                    let tag_target = concat!(module_path!(), "::tag");

                    log::debug!(target: tag_target, "{} - {:?}", i, tag);

                    match tag {
                        Ok(Tag::Title(title)) => new_tags.title = Some(title),
                        Ok(Tag::Organisation(organisation)) => {
                            new_tags.organisation = Some(organisation)
                        }
                        Ok(Tag::Artist(artist)) => new_tags.artist = Some(artist),
                        Ok(Tag::Album(album)) => new_tags.album = Some(album),
                        Ok(Tag::Genre(genre)) => new_tags.genre = Some(genre),
                        Ok(Tag::Image(image)) => new_tags.image = Some(image.into_inner()),
                        Ok(Tag::Comment(comment)) => new_tags.comment = Some(comment),
                        Ok(Tag::Unknown { .. }) => {}
                        Err(err) => {
                            self.broadcast_error_message(
                                rradio_messages::TagError(format!("{:#}", err).into()).into(),
                            );
                        }
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
            MessageView::StateChanged(state_change) => {
                if self.playbin.is_src_of(unsafe { *state_change.as_ptr() }) {
                    let new_state = state_change.get_current();

                    self.published_state.pipeline_state =
                        super::playbin::gstreamer_state_to_pipeline_state(new_state)?;

                    self.broadcast_state_change();

                    let state_change_target = concat!(module_path!(), "::state_change");

                    log::debug!(target: state_change_target, "{:?}", new_state);
                }
                Ok(())
            }
            MessageView::Eos(..) => {
                let end_of_stream_target = concat!(module_path!(), "::end_of_stream");
                log::debug!(target: end_of_stream_target, "");

                if self.current_playlist.is_some() {
                    if self.published_state.track_duration.is_some() {
                        self.goto_next_track().await
                    } else {
                        let current_playlist =
                            self.current_playlist.as_mut().ok_or(Error::NoPlaylist)?;

                        let pause_before_playing =
                            current_playlist.pause_before_playing.unwrap_or_default()
                                + self.config.pause_before_playing_increment;

                        current_playlist.pause_before_playing = Some(pause_before_playing);
                        self.published_state.pause_before_playing = Some(pause_before_playing);

                        if pause_before_playing > self.config.max_pause_before_playing {
                            Err(rradio_messages::PipelineError(
                                "Max pause_before_playing timeout exceeded".into(),
                            )
                            .into())
                        } else {
                            self.play_current_track().await
                        }
                    }
                } else {
                    self.playbin
                        .set_pipeline_state(PipelineState::Null)
                        .map_err(Error::from)
                }
            }
            MessageView::Error(err) => {
                let glib_err = err.get_error();
                let prefix = if glib_err.is::<gstreamer::ResourceError>() {
                    "Resource not found: "
                } else {
                    ""
                };
                let message = format!("{}{} ({:?})", prefix, err.get_error(), err.get_debug());
                let error = Error::PipelineError(PipelineError(message.into()));
                if self.config.play_error_sound_on_gstreamer_error {
                    self.broadcast_error_message(error);
                    Ok(())
                } else {
                    Err(error)
                }
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
pub fn run(
    config: Config,
) -> anyhow::Result<(
    impl std::future::Future<Output = ()>,
    PartialPortChannels<()>,
)> {
    gstreamer::init()?;
    let playbin = Playbin::new(&config)?;
    let bus = playbin.bus()?;

    if let Some(url) = &config.notifications.ready {
        if let Some(err) = playbin.play_url(url).err() {
            log::error!("{:#}", err);
        }
    }

    let (commands_tx, commands_rx) = mpsc::unbounded_channel();

    let published_state = PlayerState {
        pipeline_state: playbin.pipeline_state().unwrap_or(PipelineState::Null),
        current_station: Arc::new(None),
        pause_before_playing: None,
        current_track_index: 0,
        current_track_tags: Arc::new(None),
        volume: playbin.volume().unwrap_or_default(),
        buffering: 0,
        track_duration: None,
        track_position: None,
        ping_times: rradio_messages::PingTimes::None,
    };

    let (new_state_tx, new_state_rx) = watch::channel(published_state.clone());

    let (log_message_tx, _) = broadcast::channel(16);

    let log_message_source = LogMessageSource(log_message_tx.clone());

    #[cfg(feature = "ping")]
    let (ping_task, ping_requests_tx, ping_times_rx) = super::ping::run(&config)?;

    let mut controller = Controller {
        config,
        playbin,
        current_playlist: None,
        published_state,
        new_state_tx,
        log_message_tx,
        #[cfg(feature = "ping")]
        ping_requests_tx,
    };

    let task = async move {
        use futures::StreamExt;

        #[cfg(feature = "ping")]
        let ping_handle = tokio::spawn(ping_task);

        let commands = futures::stream::unfold(commands_rx, |mut rx| async {
            let message = Message::Command(rx.recv().await?);
            Some((message, rx))
        });

        let bus_stream = bus.stream().map(Message::FromGStreamer);

        #[cfg(feature = "ping")]
        let messages = {
            let ping_stream = futures::stream::unfold(ping_times_rx, |mut rx| async {
                let ping_times = rx.recv().await?;
                Some((Message::PingTimes(ping_times), rx))
            });

            futures::stream::select_all(vec![
                commands.boxed(),
                bus_stream.boxed(),
                ping_stream.boxed(),
            ])
        };

        #[cfg(not(feature = "ping"))]
        let messages = futures::stream::select(commands, bus_stream);

        tokio::pin!(messages);

        let timeout = Duration::from_millis(1000 / 3);

        loop {
            match tokio::time::timeout(timeout, messages.next()).await {
                Ok(None) => break,
                Ok(Some(message)) => {
                    if let Err(err) = match message {
                        Message::Command(command) => controller.handle_command(command).await,
                        Message::FromGStreamer(message) => {
                            controller.handle_gstreamer_message(&message).await
                        }
                        #[cfg(feature = "ping")]
                        Message::PingTimes(ping_times) => {
                            controller.handle_ping_times(ping_times);
                            Ok(())
                        }
                    } {
                        controller.broadcast_error_message(err);
                        controller.play_error();
                    }
                }
                Err(_) => controller.broadcast_state_change(),
            }
        }

        #[cfg(feature = "ping")]
        {
            drop(controller.ping_requests_tx);
            if let Err(err) = ping_handle.await {
                log::error!("Error with ping routine: {}", err);
            }
        }
    };

    Ok((
        task,
        PartialPortChannels {
            commands: commands_tx,
            player_state: new_state_rx,
            log_message_source,
            shutdown_signal: (),
        },
    ))
}
