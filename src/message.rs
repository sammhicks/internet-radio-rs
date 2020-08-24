use std::sync::Arc;

#[derive(Debug)]
pub enum Command {
    SetChannel(String),
    PlayPause,
    PreviousItem,
    NextItem,
    VolumeUp,
    VolumeDown,
    #[cfg(feature = "web_interface")]
    SetVolume(i32),
    #[cfg(feature = "web_interface")]
    PlayUrl(String),
}

#[derive(Clone, Debug, Default, serde::Serialize, PartialEq)]
pub struct TrackTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub image: Option<String>,
}

#[derive(Copy, Clone, PartialEq)]
pub struct PipelineState(gstreamer::State);

impl PipelineState {
    pub fn is_playing(self) -> bool {
        self.0 == gstreamer::State::Playing
    }
}

impl std::fmt::Debug for PipelineState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::fmt::Display for PipelineState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&self, f)
    }
}

impl std::convert::From<gstreamer::State> for PipelineState {
    fn from(state: gstreamer::State) -> Self {
        Self(state)
    }
}

#[derive(Clone, Debug)]
pub struct PlayerState {
    pub pipeline_state: PipelineState,
    pub current_track: Arc<Option<TrackTags>>,
    pub volume: i32,
    pub buffering: u8,
}
