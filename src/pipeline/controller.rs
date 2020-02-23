use actix::prelude::*;
use anyhow::Result;
use gstreamer::prelude::*;
use log::{debug, error, info, warn};

use super::playbin::Playbin;
use crate::{channel::Channel, config::Config, message::Command, tag::Tag};

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

pub struct Controller {
    config: Config,
    playbin: Playbin,
    bus: gstreamer::Bus,
    current_channel: Option<ChannelState>,
}

impl Controller {
    pub fn new(config: Config) -> Result<Self> {
        let playbin = Playbin::new()?;
        let bus = playbin.get_bus()?;

        Ok(Self {
            config,
            playbin,
            bus,
            current_channel: None,
        })
    }

    fn play_pause(&mut self) -> Result<()> {
        if self.current_channel.is_some() {
            self.playbin.play_pause()
        } else {
            Ok(())
        }
    }

    fn goto_previous_track(&mut self) -> Result<()> {
        if let Some(current_channel) = &mut self.current_channel {
            current_channel.goto_previous_track(&self.playbin)
        } else {
            Ok(())
        }
    }

    fn goto_next_track(&mut self) -> Result<()> {
        if let Some(current_channel) = &mut self.current_channel {
            current_channel.goto_next_track(&self.playbin)
        } else {
            Ok(())
        }
    }

    fn play_error(&mut self) {
        self.current_channel = None;
        if let Some(url) = &self.config.notifications.error {
            if let Err(err) = self.playbin.set_url(&url) {
                error!("{:?}", err);
            }
        }
    }
}

impl Actor for Controller {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        Self::add_stream(gstreamer::BusStream::new(&self.bus), ctx);

        if let Some(url) = &self.config.notifications.success {
            if let Err(err) = self.playbin.set_url(&url) {
                error!("{:?}", err);
            }
        }
    }
}

impl Handler<Command> for Controller {
    type Result = ();

    fn handle(&mut self, command: Command, _ctx: &mut Self::Context) {
        if let Err(err) = match command {
            Command::SetChannel(index) => {
                if let Err(err) = Channel::load(&self.config.channels_directory, index)
                    .map(|new_channel| {
                        new_channel
                            .start_with_notification(self.config.notifications.success.clone())
                    })
                    .and_then(|new_channel| match new_channel.playlist.get(0) {
                        Some(entry) => {
                            self.current_channel = Some(ChannelState {
                                channel: new_channel.clone(),
                                index: 0,
                            });
                            self.playbin
                                .set_url(&entry.url)
                                .map(|()| info!("New Channel: {:?}", new_channel))
                        }
                        None => Err(anyhow::Error::msg("Empty Playlist")),
                    })
                {
                    warn!("{:?}", err);
                    self.play_error();
                };
                Ok(())
            }
            Command::PlayPause => self.play_pause(),
            Command::PreviousItem => self.goto_previous_track(),
            Command::NextItem => self.goto_next_track(),
            Command::VolumeUp => self
                .playbin
                .change_volume(self.config.volume_offset_percent),
            Command::VolumeDown => self
                .playbin
                .change_volume(-self.config.volume_offset_percent),
        } {
            error!("{:?}", err);
        }
    }
}

impl StreamHandler<gstreamer::message::Message> for Controller {
    fn handle(&mut self, message: gstreamer::message::Message, _ctx: &mut Self::Context) {
        use gstreamer::MessageView;
        match message.view() {
            MessageView::Buffering(b) => {
                debug!("Buffering: {}", b.get_percent());
            }
            MessageView::Tag(tag) => {
                for (name, value) in tag.get_tags().as_ref().iter() {
                    debug!("Tag: {:?}", Tag::from_value(name, &value));
                }
            }
            MessageView::StateChanged(state_change) => {
                if self.playbin.is_src_of(unsafe { *state_change.as_ptr() }) {
                    info!("{:?}", state_change.get_current());
                }
            }
            MessageView::Eos(..) => {
                if let Err(err) = self.goto_next_track() {
                    error!("{:?}", err);
                    self.play_error();
                }
            }
            MessageView::Error(err) => {
                let glib_err = err.get_error();
                if glib_err.is::<gstreamer::ResourceError>() {
                    error!("Resource not found: {:?}", err.get_error().to_string());
                    return;
                }
                error!("{} ({:?})", err.get_error(), err.get_debug());
                self.play_error();
            }
            _ => (),
        }
    }
}
