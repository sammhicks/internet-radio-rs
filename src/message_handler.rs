use std::string::ToString;

use anyhow::Result;
use futures::StreamExt;
use glib::value::SendValue;
use gstreamer::{MessageView, State};
use log::debug;

use crate::error_handler::ErrorHandler;
use crate::event::{Event, EventSender, Percent};
use crate::print_value::value_to_string;
use crate::tag::Tag;

fn get_tag(name: &str, value: SendValue) -> Result<Tag> {
    use anyhow::Error;
    match name {
        "title" => value.get()?.ok_or(Error::msg("No Value")).map(Tag::Title),
        "artist" => value.get()?.ok_or(Error::msg("No Value")).map(Tag::Artist),
        "album" => value.get()?.ok_or(Error::msg("No Value")).map(Tag::Album),
        "genre" => value.get()?.ok_or(Error::msg("No Value")).map(Tag::Genre),
        _ => Ok(Tag::Unknown {
            name: name.into(),
            value: value_to_string(&value)?,
        }),
    }
}

fn handle_gstreamer_error(err: gstreamer::message::Error) -> Event {
    let glib_err = err.get_error();

    if glib_err.is::<gstreamer::ResourceError>() {
        return Event::ResourceNotFound(err.get_error().to_string());
    }

    Event::Error(format!(
        "Unknown Error: {} ({:?})",
        err.get_error(),
        err.get_debug()
    ))
}

pub async fn main(bus: gstreamer::Bus, channel: EventSender) -> Result<()> {
    let mut error_handler = ErrorHandler::new(channel.clone());

    let mut messages = gstreamer::BusStream::new(&bus);

    let mut previous_state_change = None;

    while let Some(message) = messages.next().await {
        match message.view() {
            MessageView::Buffering(b) => {
                channel.send(Event::Buffering(Percent(b.get_percent() as u8)))?
            }
            MessageView::Tag(tag) => {
                for (name, value) in tag.get_tags().as_ref().iter() {
                    if let Some(tag) = error_handler.handle(get_tag(name, value)) {
                        channel.send(Event::Tag(tag))?
                    }
                }
            }
            MessageView::StateChanged(state_change) => {
                let new_state = state_change.get_current();

                if previous_state_change != Some(new_state) {
                    match new_state {
                        State::Playing => channel.send(Event::Playing)?,
                        State::Paused => channel.send(Event::Paused)?,
                        _ => (),
                    };
                    previous_state_change = Some(new_state);
                }
            }
            MessageView::Eos(_) => channel.send(Event::EndOfStream)?,
            MessageView::NewClock(_) => (),
            MessageView::DurationChanged(_) => (),
            MessageView::NeedContext(_) => (),
            MessageView::HaveContext(_) => (),
            MessageView::Latency(_) => (),
            MessageView::AsyncStart(_) => (),
            MessageView::AsyncDone(_) => (),
            MessageView::StreamStart(_) => (),
            MessageView::StreamStatus(_) => (),
            MessageView::Element(_) => (),
            MessageView::Qos(_) => (),
            MessageView::Error(err) => channel.send(handle_gstreamer_error(err))?,
            msg => channel.send(Event::Error(format!("Unknown Message: {:?}", msg)))?,
        }
    }

    debug!("message_handler finished");

    Ok(())
}
