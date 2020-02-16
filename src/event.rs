use crate::channel;

#[derive(Debug)]
pub struct Percent(pub i32);

#[derive(Debug)]
pub enum Event {
    Playing,
    Paused,
    EndOfStream,
    Buffering(Percent),
    Tag(crate::tag::Tag),
    NewChannel(channel::Channel),
    ResourceNotFound(String),
    Error(String),
}

pub type Sender = tokio::sync::mpsc::UnboundedSender<Event>;
pub type Receiver = tokio::sync::mpsc::UnboundedReceiver<Event>;
