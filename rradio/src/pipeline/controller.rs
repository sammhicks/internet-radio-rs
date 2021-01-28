use std::sync::Arc;
use std::{convert::TryInto, time::Duration};
use tokio::sync::{broadcast, mpsc, watch};

use rradio_messages::{Command, LogMessage, PipelineError, TrackTags};

use super::playbin::{PipelineState, Playbin};
use crate::{
    atomic_string::AtomicString,
    config::Config,
    errors::{Error, Result},
    ports::PartialPortChannels,
    station::{Station, Track},
    tag::Tag,
};

#[derive(Clone, Debug)]
pub struct LogMessageSource(broadcast::Sender<LogMessage<AtomicString>>);

impl LogMessageSource {
    pub fn subscribe(&self) -> broadcast::Receiver<LogMessage<AtomicString>> {
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
    fn current_track(&self) -> Result<&Track> {
        self.tracks
            .get(self.current_track_index)
            .ok_or(rradio_messages::Error::InvalidTrackIndex(
                self.current_track_index,
            ))
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
    pub current_station: Arc<Option<rradio_messages::Station<AtomicString, Arc<[Track]>>>>,
    pub pause_before_playing: Option<Duration>,
    pub current_track_index: usize,
    pub current_track_tags: Arc<Option<TrackTags<AtomicString>>>,
    pub volume: i32,
    pub buffering: u8,
    pub track_duration: Option<Duration>,
    pub track_position: Option<Duration>,
}

struct Controller {
    config: Config,
    playbin: Playbin,
    current_playlist: Option<PlaylistState>,
    published_state: PlayerState,
    new_state_tx: watch::Sender<PlayerState>,
    log_message_tx: broadcast::Sender<LogMessage<AtomicString>>,
}

impl Controller {
    fn play_pause(&mut self) -> Result<()> {
        if self.current_playlist.is_some() {
            self.playbin.play_pause().map_err(Error::from)
        } else {
            Ok(())
        }
    }

    async fn play_current_track(&mut self) -> Result<()> {
        let current_playlist = self
            .current_playlist
            .as_ref()
            .ok_or(rradio_messages::Error::NoPlaylist)?;

        let track = current_playlist.current_track()?;
        let pause_before_playing = current_playlist.pause_before_playing;

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
        Ok(())
    }

    async fn goto_previous_track(&mut self) -> Result<()> {
        self.current_playlist
            .as_mut()
            .ok_or(rradio_messages::Error::NoPlaylist)?
            .goto_previous_track();
        self.play_current_track().await
    }

    async fn goto_next_track(&mut self) -> Result<()> {
        self.current_playlist
            .as_mut()
            .ok_or(rradio_messages::Error::NoPlaylist)?
            .goto_next_track();
        self.play_current_track().await
    }

    fn play_error(&mut self) {
        self.current_playlist = None;
        if let Some(url) = &self.config.notifications.error {
            if let Err(err) = self.playbin.play_url(&url) {
                log::error!("{:#}", err);
            }
        } else {
            self.playbin.set_pipeline_state(PipelineState::Null).ok();
        }
    }

    fn broadcast_state_change(&mut self) {
        self.published_state.track_duration = self.playbin.duration();
        self.published_state.track_position = self.playbin.position();

        self.new_state_tx.send(self.published_state.clone()).ok();
    }

    fn broadcast_error_message(&mut self, error: crate::errors::Error) {
        log::error!("{}", error);
        self.log_message_tx.send(error.into()).ok();
    }

    async fn play_station(&mut self, new_station: Station) -> Result<()> {
        self.current_playlist.take();

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

    fn set_volume(&mut self, volume: i32) -> Result<()> {
        self.published_state.volume = self.playbin.set_volume(volume)?;
        self.broadcast_state_change();
        Ok(())
    }

    fn change_volume(&mut self, direction: i32) -> Result<()> {
        let current_volume = self.playbin.volume()?;

        #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
        let rounded_volume = self.config.volume_offset
            * ((current_volume as f32) / (self.config.volume_offset as f32)).round() as i32;

        self.set_volume(rounded_volume + direction * self.config.volume_offset)
    }

    async fn handle_command(&mut self, command: Command) -> Result<()> {
        match command {
            Command::SetChannel(index) => {
                let new_station = Station::load(&self.config.stations_directory, index)?;
                self.play_station(new_station).await
            }
            Command::PlayPause => self.play_pause(),
            Command::PreviousItem => self.goto_previous_track().await,
            Command::NextItem => self.goto_next_track().await,
            Command::SeekTo(_position) => {
                log::info!("Ignoring Seek");
                Ok(())
            }
            Command::VolumeUp => self.change_volume(1),
            Command::VolumeDown => self.change_volume(-1),
            Command::SetVolume(volume) => self.set_volume(volume),
            Command::PlayUrl(url) => self.play_station(Station::singleton(url)).await,
            Command::Eject => {
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
    async fn handle_gstreamer_message(&mut self, message: &gstreamer::Message) -> Result<()> {
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
                let mut new_tags = TrackTags::<AtomicString>::default();

                for (i, (name, value)) in tag.get_tags().as_ref().iter().enumerate() {
                    let tag = Tag::from_value(name, &value);
                    let tag_target = concat!(module_path!(), "::tag");

                    log::debug!(target: tag_target, "{} - {:?}", i, tag);

                    match tag {
                        Ok(Tag::Title(title)) => new_tags.title = Some(title.into()),
                        Ok(Tag::Organisation(organisation)) => {
                            new_tags.organisation = Some(organisation.into())
                        }
                        Ok(Tag::Artist(artist)) => new_tags.artist = Some(artist.into()),
                        Ok(Tag::Album(album)) => new_tags.album = Some(album.into()),
                        Ok(Tag::Genre(genre)) => new_tags.genre = Some(genre.into()),
                        Ok(Tag::Image(image)) => new_tags.image = Some(image.into_inner().into()),
                        Ok(Tag::Comment(comment)) => new_tags.comment = Some(comment.into()),
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
                        let current_playlist = self
                            .current_playlist
                            .as_mut()
                            .ok_or(rradio_messages::Error::NoPlaylist)?;

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
                        .map_err(rradio_messages::Error::from)
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
                Err(PipelineError(message.into()).into())
            }
            _ => Ok(()),
        }
    }
}

enum Message {
    Command(Command),
    GStreamerMessage(gstreamer::Message),
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
    };

    let (new_state_tx, new_state_rx) = watch::channel(published_state.clone());

    let (log_message_tx, _) = broadcast::channel(16);

    let log_message_source = LogMessageSource(log_message_tx.clone());

    let mut controller = Controller {
        config,
        playbin,
        current_playlist: None,
        published_state,
        new_state_tx,
        log_message_tx,
    };

    let task = async move {
        use futures::StreamExt;

        let commands = futures::stream::unfold(commands_rx, |mut rx| async {
            let message = Message::Command(rx.recv().await?);
            Some((message, rx))
        });

        tokio::pin!(commands);

        let bus_stream = bus.stream().map(Message::GStreamerMessage);

        let mut messages = futures::stream::select(commands, bus_stream);

        let timeout = Duration::from_millis(1000 / 3);

        loop {
            match tokio::time::timeout(timeout, messages.next()).await {
                Ok(None) => break,
                Ok(Some(message)) => {
                    if let Err(err) = match message {
                        Message::Command(command) => controller.handle_command(command).await,
                        Message::GStreamerMessage(message) => {
                            controller.handle_gstreamer_message(&message).await
                        }
                    } {
                        controller.broadcast_error_message(err);
                        controller.play_error();
                    }
                }
                Err(_) => controller.broadcast_state_change(),
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
