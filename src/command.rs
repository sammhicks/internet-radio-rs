use crate::channel;

#[derive(Debug)]
pub enum Command {
    PlayPause,
    SetChannel(channel::Index),
}

pub type Sender = tokio::sync::mpsc::UnboundedSender<Command>;
pub type Receiver = tokio::sync::mpsc::UnboundedReceiver<Command>;
