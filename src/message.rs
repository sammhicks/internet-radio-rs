#[derive(Debug)]
pub enum Command {
    SetChannel(String),
    PlayPause,
    PreviousItem,
    NextItem,
    VolumeUp,
    VolumeDown,
    SetVolume(i32),
}

#[derive(Debug)]
pub enum StateChange {
    VolumeChanged(i32),
}
