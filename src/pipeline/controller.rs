use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

use super::playbin::{Playbin, State as PipelineState};
use crate::{
    config::Config,
    message::{Command, PlayerState},
    station::{Station, Track},
    tag::Tag,
};

struct StationState {
    station: Arc<Station>,
    tracks: Vec<Track>,
    current_track_index: usize,
}

impl StationState {
    fn current_track(&self) -> Option<&Track> {
        self.tracks.get(self.current_track_index)
    }

    fn goto_previous_track(&mut self, playbin: &Playbin) -> Result<()> {
        self.current_track_index = if self.current_track_index == 0 {
            self.tracks.len() - 1
        } else {
            self.current_track_index - 1
        };

        self.update_url(playbin)
    }

    fn goto_next_track(&mut self, playbin: &Playbin) -> Result<()> {
        self.current_track_index += 1;
        if self.current_track_index == self.tracks.len() {
            self.current_track_index = 0;
        }

        self.update_url(playbin)
    }

    fn update_url(&self, playbin: &Playbin) -> Result<()> {
        use anyhow::Context;
        self.current_track()
            .context("Failed to get playlist item")
            .and_then(|track| playbin.set_url(&track.url))
    }
}

struct Controller {
    config: Config,
    playbin: Playbin,
    current_station: Option<StationState>,
    current_state: PlayerState,
    new_state_tx: Option<watch::Sender<PlayerState>>,
}

impl Controller {
    fn play_pause(&mut self) -> Result<()> {
        if self.current_station.is_none() {
            return Ok(());
        }
        self.playbin.play_pause()
    }

    fn goto_previous_track(&mut self) -> Result<()> {
        if let Some(current_station) = &mut self.current_station {
            current_station.goto_previous_track(&self.playbin)?;
            self.broadcast_new_track();
        }
        Ok(())
    }

    fn goto_next_track(&mut self) -> Result<()> {
        if let Some(current_station) = &mut self.current_station {
            current_station.goto_next_track(&self.playbin)?;
            self.broadcast_new_track();
        }
        Ok(())
    }

    fn play_error(&mut self) {
        self.current_station = None;
        if let Some(url) = &self.config.notifications.error {
            if let Err(err) = self.playbin.set_url(&url) {
                log::error!("{:?}", err);
            }
        }
    }

    fn broadcast_new_track(&mut self) {
        self.current_state.current_track_tags = Arc::new(None);
        self.broadcast_state_change();
    }

    fn broadcast_state_change(&mut self) {
        if let Some(new_state_tx) = &self.new_state_tx {
            if new_state_tx.broadcast(self.current_state.clone()).is_err() {
                self.new_state_tx = None;
            }
        }
    }

    fn handle_volume_change(&mut self, new_volume: i32) {
        self.current_state.volume = new_volume;
        self.broadcast_state_change();
    }

    fn play_station(&mut self, new_station: Station) -> Result<()> {
        // Add the notification track if it exists
        let mut tracks: Vec<_> = self
            .config
            .notifications
            .success
            .iter()
            .cloned()
            .map(Track::notification)
            .collect();
        // Add the station tracks
        tracks.append(&mut new_station.tracks()?);

        match tracks.get(0) {
            Some(entry) => {
                self.playbin.set_url(&entry.url)?;
                self.current_station = Some(StationState {
                    station: Arc::new(new_station),
                    tracks,
                    current_track_index: 0,
                });
                self.current_state.current_track_tags = Arc::new(None);
                self.broadcast_state_change();
                Ok(())
            }
            None => Err(anyhow::Error::msg("Empty Playlist")),
        }
    }

    fn handle_command(&mut self, command: Command) {
        if let Err(err) = match command {
            Command::SetChannel(index) => {
                if let Err(err) = Station::load(&self.config.stations_directory, index)
                    .and_then(|new_station| self.play_station(new_station))
                {
                    log::warn!("{:?}", err);
                    self.play_error();
                };
                Ok(())
            }
            Command::PlayPause => self.play_pause(),
            Command::PreviousItem => self.goto_previous_track(),
            Command::NextItem => self.goto_next_track(),
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
            Command::PlayUrl(url) => self.play_station(Station::singleton(url)),
        } {
            log::error!("{:?}", err);
        }
    }

    fn handle_gstreamer_message(&mut self, message: &gstreamer::Message) {
        use gstreamer::MessageView;
        match message.view() {
            MessageView::Buffering(b) => {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

                log::debug!("Buffering: {}", b.get_percent());

                self.current_state.buffering = b.get_percent() as u8;

                self.broadcast_state_change();
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
                        Err(err) => log::error!("{:?}", err),
                    }
                }
                let should_display_tags = self
                    .current_station
                    .as_ref()
                    .and_then(|station| station.current_track().map(|entry| !entry.is_notification))
                    .unwrap_or(false);
                if should_display_tags && new_tags != crate::message::TrackTags::default() {
                    self.current_state.current_track_tags = Arc::new(Some(new_tags));
                    self.broadcast_state_change();
                }
            }
            MessageView::StateChanged(state_change) => {
                if self.playbin.is_src_of(unsafe { *state_change.as_ptr() }) {
                    let new_state = state_change.get_current();

                    self.current_state.pipeline_state = new_state;

                    self.broadcast_state_change();

                    log::info!("{:?}", new_state);
                }
            }
            MessageView::Eos(..) => {
                if let Err(err) = self.goto_next_track() {
                    log::error!("{:?}", err);
                    self.play_error();
                }
            }
            MessageView::Error(err) => {
                let glib_err = err.get_error();
                if glib_err.is::<gstreamer::ResourceError>() {
                    log::error!("Resource not found: {:?}", err.get_error().to_string());
                }
                log::error!("{} ({:?})", err.get_error(), err.get_debug());
                self.play_error();
            }
            _ => (),
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
        if let Some(err) = playbin.set_url(url).err() {
            log::error!("{:?}", err);
        }
    }

    let current_state = PlayerState {
        pipeline_state: playbin.pipeline_state().unwrap_or(PipelineState::Null),
        current_station: None,
        current_track_tags: Arc::new(None),
        volume: playbin.volume().unwrap_or_default(),
        buffering: 0,
    };

    let (new_state_tx, new_state_rx) = watch::channel(current_state.clone());

    let mut controller = Controller {
        config,
        playbin,
        current_station: None,
        current_state,
        new_state_tx: Some(new_state_tx),
    };

    let task = async move {
        use tokio::stream::StreamExt;

        let commands = commands.map(Message::Command);

        let bus_stream = bus.stream().map(Message::GStreamerMessage);

        let mut messages = commands.merge(bus_stream);

        while let Some(message) = messages.next().await {
            match message {
                Message::Command(command) => controller.handle_command(command),
                Message::GStreamerMessage(message) => controller.handle_gstreamer_message(&message),
            }
        }

        Ok(())
    };

    Ok((task, new_state_rx))
}
