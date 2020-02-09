use log::{debug, Level};

use crate::event::{Event, EventReciever};
use crate::tag::Tag;

pub async fn main(mut channel: EventReciever) {
    while let Some(event) = channel.recv().await {
        let level = match event {
            Event::Buffering(..) => Level::Debug,
            Event::Tag(Tag::Unknown { .. }) => Level::Debug,
            Event::PartialChannel(..) => Level::Debug,
            Event::ChannelCancelled => Level::Debug,
            Event::Error(..) => Level::Error,
            _ => Level::Info,
        };

        log::log!(level, "Event: {:?}", event);
    }

    debug!("event_logger finished");
}
