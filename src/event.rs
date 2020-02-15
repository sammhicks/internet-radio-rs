use crate::channel;

#[derive(Debug)]
pub struct Percent(pub u8);

#[derive(Debug)]
pub enum Event {
    Playing,
    Paused,
    EndOfStream,
    Buffering(Percent),
    Tag(crate::tag::Tag),
    NewChannel(channel::Channel),
    ChannelNotFound(channel::Index),
    ResourceNotFound(String),
    Error(String),
}

pub type Sender = tokio::sync::mpsc::UnboundedSender<Event>;
pub type Receiver = tokio::sync::mpsc::UnboundedReceiver<Event>;
