use std::sync::Arc;

#[derive(Debug)]
pub enum Command {
    SetChannel(String),
    PlayPause,
    PreviousItem,
    NextItem,
    VolumeUp,
    VolumeDown,
    SetVolume(i32),
    PlayUrl(String),
}

#[derive(Clone, Debug, Default, serde::Serialize, PartialEq)]
pub struct TrackTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub image: Option<Arc<String>>,
}

#[derive(Clone, Debug)]
pub struct PlayerState {
    pub pipeline_state: Arc<String>,
    pub current_track: Arc<Option<TrackTags>>,
    pub volume: i32,
    pub buffering: u8,
}
