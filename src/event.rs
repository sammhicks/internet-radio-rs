use super::channel::{Channel, ChannelIndex};

#[derive(Debug)]
pub struct Percent(pub u8);

#[derive(Debug)]
pub enum Event {
    Playing,
    Paused,
    EndOfStream,
    Buffering(Percent),
    Tag(super::tag::Tag),
    PartialChannel(ChannelIndex),
    ChannelCancelled,
    NewChannel(Channel),
    ChannelNotFound(ChannelIndex),
    Error(String),
}

pub type EventSender = tokio::sync::mpsc::UnboundedSender<Event>;
pub type EventReciever = tokio::sync::mpsc::UnboundedReceiver<Event>;
