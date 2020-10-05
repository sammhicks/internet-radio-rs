use anyhow::{Context, Result};
use std::convert::TryInto;
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

use super::playbin::{Playbin, State as PipelineState};
use crate::{
    config::Config,
    message::{Command, PlayerState},
    station::{Station, Track},
    tag::Tag,
};

struct PlaylistState {
    station: Arc<Station>,
    tracks: Vec<Track>,
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

struct Controller {
    config: Config,
    playbin: Playbin,
    current_playlist: Option<PlaylistState>,
    published_state: PlayerState,
    new_state_tx: Option<watch::Sender<PlayerState>>,
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
        let pause_before_playing = current_playlist.station.pause_before_playing();

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
        if let Some(new_state_tx) = &self.new_state_tx {
            if new_state_tx
                .broadcast(self.published_state.clone())
                .is_err()
            {
                self.new_state_tx = None;
            }
        }
    }

    fn handle_volume_change(&mut self, new_volume: i32) {
        self.published_state.volume = new_volume;
        self.broadcast_state_change();
    }

    async fn play_station(&mut self, new_station: Station) -> Result<()> {
        let tracks = new_station.tracks()?;

        log::debug!("Station tracks: {:?}", tracks);

        let success_notification = self
            .config
            .notifications
            .success
            .iter()
            .cloned()
            .map(Track::notification);

        let playlist_tracks = success_notification.chain(tracks.iter().cloned()).collect();

        self.current_playlist = Some(PlaylistState {
            station: Arc::new(new_station),
            tracks: playlist_tracks,
            current_track_index: 0,
        });

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
            Command::VolumeUp => self
                .playbin
                .change_volume(self.config.volume_offset_percent)
                .map(|new_volume| self.handle_volume_change(new_volume)),
            Command::VolumeDown => self
                .playbin
                .change_volume(-self.config.volume_offset_percent)
                .map(|new_volume| self.handle_volume_change(new_volume)),
            #[cfg(feature = "web_interface")]
            Command::SetVolume(volume) => self
                .playbin
                .set_volume(volume)
                .map(|new_volume| self.handle_volume_change(new_volume)),
            #[cfg(feature = "web_interface")]
            Command::PlayUrl(url) => self.play_station(Station::singleton(url)).await,
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
                let mut new_tags = crate::message::TrackTags::default();

                for (i, (name, value)) in tag.get_tags().as_ref().iter().enumerate() {
                    let tag = Tag::from_value(name, &value);

                    log::debug!(
                        target: concat!(module_path!(), "::tag"),
                        "{} - {:?}",
                        i,
                        tag
                    );

                    match tag {
                        Ok(Tag::Title(title)) => new_tags.title = Some(title),
                        Ok(Tag::Artist(artist)) => new_tags.artist = Some(artist),
                        Ok(Tag::Album(album)) => new_tags.album = Some(album),
                        Ok(Tag::Genre(genre)) => new_tags.genre = Some(genre),
                        Ok(Tag::Image(image)) => new_tags.image = Some(image.unwrap()),
                        Ok(Tag::Unknown { .. }) => (),
                        Err(err) => log::error!("{:#}", err),
                    }
                }

                if let Some(playlist_state) = &self.current_playlist {
                    if let Ok(track) = playlist_state.current_track() {
                        if track.is_notification && new_tags != crate::message::TrackTags::default()
                        {
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

                    self.published_state.pipeline_state = new_state;

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
                if let Some(resource_err) = glib_err.kind::<gstreamer::ResourceError>() {
                    log::error!("{:?}", resource_err);
                }
                let prefix = if glib_err.is::<gstreamer::ResourceError>() {
                    "Resource not found: "
                } else {
                    ""
                };
                anyhow::bail!("{}{} ({:?})", prefix, err.get_error(), err.get_debug());
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
    impl std::future::Future<Output = Result<(), anyhow::Error>>,
    watch::Receiver<PlayerState>,
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
        current_station: None,
        current_track_tags: Arc::new(None),
        volume: playbin.volume().unwrap_or_default(),
        buffering: 0,
    };

    let (new_state_tx, new_state_rx) = watch::channel(published_state.clone());

    let mut controller = Controller {
        config,
        playbin,
        current_playlist: None,
        published_state,
        new_state_tx: Some(new_state_tx),
    };

    let task = async move {
        use tokio::stream::StreamExt;

        let commands = commands.map(Message::Command);

        let bus_stream = bus.stream().map(Message::GStreamerMessage);

        let mut messages = commands.merge(bus_stream);

        while let Some(message) = messages.next().await {
            if let Err(err) = match message {
                Message::Command(command) => controller.handle_command(command).await,
                Message::GStreamerMessage(message) => {
                    controller.handle_gstreamer_message(&message).await
                }
            } {
                log::error!("{:#}", err);
                controller.play_error();
            }
        }

        Ok(())
    };

    Ok((task, new_state_rx))
}
