use anyhow::{Context, Result};
use futures::StreamExt;
use glib::object::ObjectExt;
use gstreamer::{Element, ElementExtManual};

pub async fn main(playbin: &Element) -> Result<()> {
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
