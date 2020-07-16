use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

use super::playbin::Playbin;
use crate::{
    channel::Channel,
    config::Config,
    message::{Command, PlayerState},
    tag::Tag,
};

struct ChannelState {
    channel: Channel,
    index: usize,
}

impl ChannelState {
    fn goto_previous_track(&mut self, playbin: &Playbin) -> Result<()> {
        self.index = if self.index == 0 {
            self.channel.playlist.len() - 1
        } else {
            self.index - 1
        };

        self.update_url(playbin)
    }

    fn goto_next_track(&mut self, playbin: &Playbin) -> Result<()> {
        self.index += 1;
        if self.index == self.channel.playlist.len() {
            self.index = 0;
        }

        self.update_url(playbin)
    }

    fn update_url(&self, playbin: &Playbin) -> Result<()> {
        use anyhow::Context;
        self.channel
            .playlist
            .get(self.index)
            .context("Failed to get playlist item")
            .and_then(|entry| playbin.set_url(&entry.url))
    }
}

struct Controller {
    config: Config,
    playbin: Playbin,
    current_channel: Option<ChannelState>,
    current_state: PlayerState,
    new_state_tx: Option<watch::Sender<PlayerState>>,
}

impl Controller {
    fn play_pause(&mut self) -> Result<()> {
        if self.current_channel.is_some() {
            self.playbin.play_pause()
        } else {
            Ok(())
        }
    }

    fn goto_previous_track(&mut self) -> Result<()> {
        if let Some(current_channel) = &mut self.current_channel {
            current_channel.goto_previous_track(&self.playbin)?;
            self.broadcast_new_track();
        }
        Ok(())
    }

    fn goto_next_track(&mut self) -> Result<()> {
        if let Some(current_channel) = &mut self.current_channel {
            current_channel.goto_next_track(&self.playbin)?;
            self.broadcast_new_track();
        }
        Ok(())
    }

    fn play_error(&mut self) {
        self.current_channel = None;
        if let Some(url) = &self.config.notifications.error {
            if let Err(err) = self.playbin.set_url(&url) {
                log::error!("{:?}", err);
            }
        }
    }

    fn broadcast_new_track(&mut self) {
        self.current_state.current_track = Arc::new(None);
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

    fn play_channel(&mut self, mut new_channel: Channel) -> Result<()> {
        new_channel.start_with_notification(self.config.notifications.success.clone());

        match new_channel.playlist.get(0) {
            Some(entry) => {
                self.current_channel = Some(ChannelState {
                    channel: new_channel.clone(),
                    index: 0,
                });
                self.current_state.current_track = Arc::new(None);
                self.broadcast_state_change();
                self.playbin
                    .set_url(&entry.url)
                    .map(|()| log::info!("New Channel: {:?}", new_channel))
            }
            None => Err(anyhow::Error::msg("Empty Playlist")),
        }
    }

    fn handle_command(&mut self, command: Command) {
        if let Err(err) = match command {
            Command::SetChannel(index) => {
                if let Err(err) = Channel::load(&self.config.channels_directory, index)
                    .and_then(|new_channel| self.play_channel(new_channel))
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
            Command::PlayUrl(url) => self.play_channel(Channel {
                index: None,
                playlist: vec![crate::playlist::Entry {
                    is_notification: false,
                    title: None,
                    url,
                }],
            }),
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
                    .current_channel
                    .as_ref()
                    .and_then(|channel| {
                        channel
                            .channel
                            .playlist
                            .get(channel.index)
                            .map(|entry| !entry.is_notification)
                    })
                    .unwrap_or(false);
                if should_display_tags && new_tags != crate::message::TrackTags::default() {
                    self.current_state.current_track = Arc::new(Some(new_tags));
                    self.broadcast_state_change();
                }
            }
            MessageView::StateChanged(state_change) => {
                if self.playbin.is_src_of(unsafe { *state_change.as_ptr() }) {
                    let new_state = state_change.get_current();

                    self.current_state.pipeline_state = new_state.into();

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

pub fn run(
    config: Config,
    commands: mpsc::UnboundedReceiver<Command>,
) -> Result<(
    impl std::future::Future<Output = Result<(), anyhow::Error>>,
    watch::Receiver<PlayerState>,
)> {
    let playbin = Playbin::new()?;
    let bus = playbin.bus()?;

    if let Some(url) = &config.notifications.success {
        if let Some(err) = playbin.set_url(url).err() {
            log::error!("{:?}", err);
        }
    }

    let current_state = playbin.state();

    let (new_state_tx, new_state_rx) = watch::channel(current_state.clone());

    let mut controller = Controller {
        config,
        playbin,
        current_channel: None,
        current_state,
        new_state_tx: Some(new_state_tx),
    };

    Ok((
        async move {
            use tokio::stream::StreamExt;

            let commands = commands.map(Message::Command);

            let bus_stream = bus.stream().map(Message::GStreamerMessage);

            let mut messages = commands.merge(bus_stream);

            while let Some(message) = messages.next().await {
                match message {
                    Message::Command(command) => controller.handle_command(command),
                    Message::GStreamerMessage(message) => {
                        controller.handle_gstreamer_message(&message)
                    }
                }
            }

            Ok(())
        },
        new_state_rx,
    ))
}
