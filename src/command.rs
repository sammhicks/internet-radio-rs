use super::channel::ChannelIndex;

#[derive(Debug)]
pub enum Command {
    PlayPause,
    PartialChannel(ChannelIndex),
    ChannelCancelled,
    SetChannel(ChannelIndex),
}

pub type CommandSender = tokio::sync::mpsc::UnboundedSender<Command>;
pub type CommandReciever = tokio::sync::mpsc::UnboundedReceiver<Command>;
