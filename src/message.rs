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

#[derive(Clone, Debug)]
pub struct PlayerState {
    pub pipeline_state: std::sync::Arc<String>,
    pub volume: i32,
    pub buffering: u8,
}
