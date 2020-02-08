#[derive(Debug)]
pub struct Percent(pub u8);

#[derive(Debug)]
pub enum Event {
    Playing,
    Paused,
    EndOfStream,
    Buffering(Percent),
    Tag(super::tag::Tag),
    Error(String),
}

pub type EventSender = tokio::sync::mpsc::UnboundedSender<Event>;
pub type EventReciever = tokio::sync::mpsc::UnboundedReceiver<Event>;
