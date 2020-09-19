//! Messages sent between asyncronous actors

use std::sync::Arc;

pub use crate::pipeline::State as PipelineState;
use crate::station::Station;

/// Commands from the user
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

#[derive(Clone, Debug)]
pub struct PlayerState {
    pub pipeline_state: PipelineState,
    pub current_station: Option<Arc<Station>>,
    pub current_track_tags: Arc<Option<TrackTags>>,
    pub volume: i32,
    pub buffering: u8,
}
