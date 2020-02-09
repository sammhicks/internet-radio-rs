use crate::channel::{Channel, ChannelIndex};

#[derive(Debug)]
pub struct Percent(pub u8);

#[derive(Debug)]
pub enum Event {
    Playing,
    Paused,
    EndOfStream,
    Buffering(Percent),
    Tag(crate::tag::Tag),
    PartialChannel(ChannelIndex),
    ChannelCancelled,
    NewChannel(Channel),
    ChannelNotFound(ChannelIndex),
    ResourceNotFound(String),
    Error(String),
}

pub type EventSender = tokio::sync::mpsc::UnboundedSender<Event>;
pub type EventReciever = tokio::sync::mpsc::UnboundedReceiver<Event>;
