use std::string::ToString;

use anyhow::Result;
use futures::StreamExt;
use glib::value::SendValue;
use gstreamer::{MessageView, State};

use crate::error_handler::ErrorHandler;
use crate::event::{self, Event, Percent};
use crate::print_value::value_to_string;
use crate::tag::Tag;

fn get_value<'v, T, F>(value: &'v SendValue, builder: F) -> Result<Tag>
where
    T: glib::value::FromValueOptional<'v>,
    F: FnOnce(T) -> Tag,
{
    use anyhow::Error;
    value
        .get()?
        .ok_or_else(|| Error::msg("No Value"))
        .map(builder)
}

fn get_tag(name: &str, value: &SendValue) -> Result<Tag> {
    match name {
        "title" => get_value(value, Tag::Title),
        "artist" => get_value(value, Tag::Artist),
        "album" => get_value(value, Tag::Album),
        "genre" => get_value(value, Tag::Genre),
        _ => Ok(Tag::Unknown {
            name: name.into(),
            value: value_to_string(value)?,
        }),
    }
}

fn handle_gstreamer_error(err: &gstreamer::message::Error) -> Event {
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

pub async fn main(pipeline: crate::playbin::Playbin, channel: event::Sender) -> Result<()> {
    let mut error_handler = ErrorHandler::new(channel.clone());

    let bus = pipeline.get_bus()?;

    let mut messages = gstreamer::BusStream::new(&bus);

    while let Some(message) = messages.next().await {
        #[allow(clippy::match_same_arms)]
        match message.view() {
            MessageView::Buffering(b) => {
                channel.send(Event::Buffering(Percent(b.get_percent())))?
            }
            MessageView::Tag(tag) => {
                for (name, value) in tag.get_tags().as_ref().iter() {
                    if let Some(tag) = error_handler.handle(get_tag(name, &value)) {
                        channel.send(Event::Tag(tag))?;
                    }
                }
            }
            MessageView::StateChanged(state_change) => {
                use gstreamer::MiniObject;
                if pipeline.is_src_of(unsafe { *state_change.as_ptr() }) {
                    match state_change.get_current() {
                        State::Playing => channel.send(Event::Playing)?,
                        State::Paused => channel.send(Event::Paused)?,
                        _ => (),
                    };
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
            MessageView::Error(err) => channel.send(handle_gstreamer_error(&err))?,
            msg => channel.send(Event::Error(format!("Unknown Message: {:?}", msg)))?,
        }
    }

    Ok(())
}
