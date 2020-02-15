#![allow(clippy::match_same_arms)]
use log::{debug, Level};

use crate::event::{Event, Reciever};
use crate::tag::Tag;

pub async fn main(mut channel: Reciever) {
    while let Some(event) = channel.recv().await {
        let level = match event {
            Event::Buffering(..) => Level::Debug,
            Event::Tag(Tag::Unknown { .. }) => Level::Debug,
            Event::ResourceNotFound(..) => Level::Error,
            Event::Error(..) => Level::Error,
            _ => Level::Info,
        };

        log::log!(level, "Event: {:?}", event);
    }

    debug!("event_logger finished");
}
