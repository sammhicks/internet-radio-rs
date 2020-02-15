#[derive(Debug)]
pub enum Command {
    PlayPause,
    SetChannel(String),
}

pub type Sender = tokio::sync::mpsc::UnboundedSender<Command>;
pub type Receiver = tokio::sync::mpsc::UnboundedReceiver<Command>;
