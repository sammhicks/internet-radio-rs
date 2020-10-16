use anyhow::{Context, Result};
use std::convert::TryInto;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, watch};

use rradio_messages::{Command, LogMessage, TrackTags};

use super::playbin::{PipelineState, Playbin};
use crate::{
    atomic_string::AtomicString,
    config::Config,
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
}

impl PlaylistState {
    fn current_track(&self) -> Result<&Track> {
        self.tracks
            .get(self.current_track_index)
            .context("Invalid track index")
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
    pub current_track_tags: Arc<Option<TrackTags<AtomicString>>>,
    pub volume: i32,
    pub buffering: u8,
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
            self.playbin.play_pause()
        } else {
            Ok(())
        }
    }

    async fn play_current_track(&mut self) -> Result<()> {
        let current_playlist = self.current_playlist.as_mut().context("No playlist")?;

        let track = current_playlist.current_track()?;
        let pause_before_playing = current_playlist.pause_before_playing;

        self.playbin.set_url(&track.url)?;
        self.published_state.current_track_tags = Arc::new(None);
        if let Some(pause_duration) = pause_before_playing {
            log::info!("Pausing for {}s", pause_duration.as_secs());
            self.playbin.set_pipeline_state(PipelineState::Paused)?;
            self.broadcast_state_change();
            tokio::time::delay_for(pause_duration).await;
        }
        self.playbin.set_pipeline_state(PipelineState::Playing)?;
        self.broadcast_state_change();
        Ok(())
    }

    async fn goto_previous_track(&mut self) -> Result<()> {
        self.current_playlist
            .as_mut()
            .context("No playlist")?
            .goto_previous_track();
        self.play_current_track().await
    }

    async fn goto_next_track(&mut self) -> Result<()> {
        self.current_playlist
            .as_mut()
            .context("No playlist")?
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
        self.new_state_tx
            .broadcast(self.published_state.clone())
            .ok();
    }

    fn broadcast_error_message(&mut self, message: String) {
        log::error!("{}", message);
        self.log_message_tx
            .send(LogMessage::error(message.into()))
            .ok();
    }

    fn handle_volume_change(&mut self, new_volume: i32) {
        self.published_state.volume = new_volume;
        self.broadcast_state_change();
    }

    async fn play_station(&mut self, new_station: Station) -> Result<()> {
        let playlist = new_station.into_playlist()?;

        log::debug!("Station tracks: {:?}", playlist.tracks);

        let success_notification = self
            .config
            .notifications
            .success
            .iter()
            .cloned()
            .map(Track::notification);

        let playlist_tracks = success_notification
            .chain(playlist.tracks)
            .collect::<Arc<_>>();

        self.current_playlist = Some(PlaylistState {
            pause_before_playing: playlist.pause_before_playing,
            tracks: playlist_tracks.clone(),
            current_track_index: 0,
        });

        self.published_state.current_station = Arc::new(Some(rradio_messages::Station {
            index: playlist.station_index.map(From::from),
            title: playlist.station_title.map(From::from),
            tracks: playlist_tracks,
        }));

        self.play_current_track().await
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
            Command::VolumeUp => self
                .playbin
                .change_volume(self.config.volume_offset_percent)
                .map(|new_volume| self.handle_volume_change(new_volume)),
            Command::VolumeDown => self
                .playbin
                .change_volume(-self.config.volume_offset_percent)
                .map(|new_volume| self.handle_volume_change(new_volume)),
            Command::SetVolume(volume) => self
                .playbin
                .set_volume(volume)
                .map(|new_volume| self.handle_volume_change(new_volume)),
            Command::PlayUrl(url) => self.play_station(Station::singleton(url)).await,
            Command::Eject => {
                log::info!("Ignoring Eject");
                Ok(())
            }
        }
    }

    #[allow(clippy::too_many_lines)]
    async fn handle_gstreamer_message(&mut self, message: &gstreamer::Message) -> Result<()> {
        use gstreamer::MessageView;
        match message.view() {
            MessageView::Buffering(b) => {
                log::debug!("Buffering: {}", b.get_percent());

                self.published_state.buffering =
                    b.get_percent().try_into().context("Bad buffering value")?;

                self.broadcast_state_change();

                Ok(())
            }
            MessageView::Tag(tag) => {
                let mut new_tags = TrackTags::<AtomicString>::default();

                for (i, (name, value)) in tag.get_tags().as_ref().iter().enumerate() {
                    let tag = Tag::from_value(name, &value);

                    log::debug!(
                        target: concat!(module_path!(), "::tag"),
                        "{} - {:?}",
                        i,
                        tag
                    );

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
                            self.broadcast_error_message(format!("{:#}", err));
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

                    log::info!("{:?}", new_state);
                }
                Ok(())
            }
            MessageView::Eos(..) => {
                if self.current_playlist.is_some() {
                    self.goto_next_track().await
                } else {
                    self.playbin.set_pipeline_state(PipelineState::Null)
                }
            }
            MessageView::Error(err) => {
                let glib_err = err.get_error();
                let prefix = if glib_err.is::<gstreamer::ResourceError>() {
                    "Resource not found: "
                } else {
                    ""
                };
                let message: AtomicString =
                    format!("{}{} ({:?})", prefix, err.get_error(), err.get_debug()).into();
                self.log_message_tx
                    .send(LogMessage::error(message.clone()))
                    .ok();
                Err(anyhow::Error::msg(message))
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
    commands: mpsc::UnboundedReceiver<Command>,
) -> Result<(
    impl std::future::Future<Output = ()>,
    watch::Receiver<PlayerState>,
    LogMessageSource,
)> {
    gstreamer::init()?;
    let playbin = Playbin::new()?;
    let bus = playbin.bus()?;

    if let Some(url) = &config.notifications.success {
        if let Some(err) = playbin.play_url(url).err() {
            log::error!("{:#}", err);
        }
    }

    let published_state = PlayerState {
        pipeline_state: playbin.pipeline_state().unwrap_or(PipelineState::Null),
        current_station: Arc::new(None),
        current_track_tags: Arc::new(None),
        volume: playbin.volume().unwrap_or_default(),
        buffering: 0,
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

        let commands = commands.map(Message::Command);

        let bus_stream = bus.stream().map(Message::GStreamerMessage);

        let mut messages = futures::stream::select(commands, bus_stream);

        while let Some(message) = messages.next().await {
            if let Err(err) = match message {
                Message::Command(command) => controller.handle_command(command).await,
                Message::GStreamerMessage(message) => {
                    controller.handle_gstreamer_message(&message).await
                }
            } {
                controller.broadcast_error_message(format!("{:#}", err));
                controller.play_error();
            }
        }
    };

    Ok((task, new_state_rx, log_message_source))
}