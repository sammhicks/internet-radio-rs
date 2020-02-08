use anyhow::{Context, Error, Result};
use futures::StreamExt;
use gstreamer::prelude::*;
use gstreamer::Element;

async fn message_loop(bus: gstreamer::Bus) -> Result<()> {
    let mut messages = gstreamer::BusStream::new(&bus);

    while let Some(message) = messages.next().await {
        use gstreamer::MessageView;

        match message.view() {
            MessageView::Buffering(buff) => {
                println!("{:?}", buff.get_percent());
            }
            MessageView::Tag(tag) => {
                println!("tags");
                for (k, v) in tag.get_tags().as_ref().iter() {
                    println!("{} => {:?}", k, v.get::<String>());
                }
            }
            MessageView::NeedContext(_) => (),
            MessageView::HaveContext(_) => (),
            MessageView::Latency(_) => (),
            MessageView::AsyncStart(_) => (),
            MessageView::AsyncDone(_) => (),
            MessageView::StateChanged(_) => (),
            MessageView::StreamStatus(_) => (),
            MessageView::Element(_) => (),
            MessageView::Qos(_) => (),
            MessageView::Eos(_) => println!("Done"),
            MessageView::Error(err) => {
                println!(
                    "Error from {:?}: {} (({:?}))",
                    err.get_src().map(|s| s.get_path_string()),
                    err.get_error(),
                    err.get_debug()
                );
            }
            msg => println!("{:?}", msg),
        }
    }

    Ok(())
}

async fn event_loop(playbin: &Element) -> Result<()> {
    use crossterm::event::{Event, EventStream, KeyCode, KeyEvent};

    let mut is_playing = true;

    let mut events = EventStream::new();
    while let Some(event) = events.next().await {
        match event? {
            Event::Key(KeyEvent {
                code: KeyCode::Esc,
                modifiers: _,
            }) => break,
            Event::Key(KeyEvent {
                code: KeyCode::Char(' '),
                modifiers: _,
            }) => {
                is_playing = if is_playing {
                    playbin
                        .set_state(gstreamer::State::Paused)
                        .context("Unable to set the pipeline to the `Paused` state")?;
                    false
                } else {
                    playbin
                        .set_state(gstreamer::State::Playing)
                        .context("Unable to set the pipeline to the `Playing` state")?;
                    true
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('1'),
                modifiers: _,
            }) => {
                playbin
                    .set_state(gstreamer::State::Null)
                    .context("Unable to set the pipeline to the `Null` state")?;
                playbin.set_property("uri", &glib::Value::from("https://open.live.bbc.co.uk/mediaselector/6/redir/version/2.0/mediaset/audio-nondrm-download/proto/https/vpid/p0824ptq.mp3"))?;
                playbin
                    .set_state(gstreamer::State::Playing)
                    .context("Unable to set the pipeline to the `Playing` state")?;
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char('2'),
                modifiers: _,
            }) => {
                playbin
                    .set_state(gstreamer::State::Null)
                    .context("Unable to set the pipeline to the `Null` state")?;
                playbin.set_property("uri", &glib::Value::from("https://www.freedesktop.org/software/gstreamer-sdk/data/media/sintel_trailer-480p.webm"))?;
                playbin
                    .set_state(gstreamer::State::Playing)
                    .context("Unable to set the pipeline to the `Playing` state")?;
            }
            e => println!("{:?}", e),
        }
    }

    Ok(())
}

struct Playbin(gstreamer::Element);

impl Playbin {
    fn new() -> Result<Self> {
        let playbin = gstreamer::ElementFactory::make("playbin", None)?;

        let flags = playbin.get_property("flags")?;
        let flags_class = glib::FlagsClass::new(flags.type_())
            .ok_or(Error::msg("Failed to create a flags class"))?;
        let flags = flags_class
            .builder_with_value(flags)
            .unwrap()
            .unset_by_nick("text")
            .unset_by_nick("video")
            .build()
            .ok_or(Error::msg("Failed to set flags"))?;
        playbin.set_property("flags", &flags)?;

        Ok(Playbin(playbin))
    }
}

impl Drop for Playbin {
    fn drop(&mut self) {
        if let Err(_) = self.0.set_state(gstreamer::State::Null) {
            println!("Unable to set the pipeline to the `Null` state");
        }
    }
}

impl std::ops::Deref for Playbin {
    type Target = gstreamer::Element;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

fn main() -> Result<()> {
    gstreamer::init()?;

    let playbin = Playbin::new()?;
    let bus = playbin.get_bus().ok_or(Error::msg("pipeline has no bus"))?;

    let mut rt = tokio::runtime::Runtime::new()?;

    rt.spawn(message_loop(bus));

    rt.block_on(event_loop(&playbin))
}
