/// Commands from the user
#[derive(Debug, serde::Deserialize, serde::Serialize)]
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

#[derive(Copy, Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum PipelineState {
    VoidPending,
    Null,
    Ready,
    Paused,
    Playing,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct Track {
    pub title: Option<String>,
    pub url: String,
    pub is_notification: bool,
}

impl Track {
    pub fn url(url: String) -> Self {
        Self {
            title: None,
            url,
            is_notification: false,
        }
    }

    pub fn notification(url: String) -> Self {
        Self {
            title: None,
            url,
            is_notification: true,
        }
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Station {
    pub index: Option<String>,
    pub title: Option<String>,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TrackTags {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub image: Option<String>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub enum OptionDiff<T> {
    NoChange,
    ChangedToNone,
    ChangedToSome(T),
}

impl<T> std::convert::From<Option<T>> for OptionDiff<T> {
    fn from(maybe_t: Option<T>) -> Self {
        match maybe_t {
            Some(t) => Self::ChangedToSome(t),
            None => Self::ChangedToNone,
        }
    }
}

impl<T> std::convert::From<Option<Option<T>>> for OptionDiff<T> {
    fn from(value: Option<Option<T>>) -> Self {
        match value {
            Some(Some(x)) => OptionDiff::ChangedToSome(x),
            Some(None) => OptionDiff::ChangedToNone,
            None => OptionDiff::NoChange,
        }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct PlayerStateDiff {
    pub pipeline_state: Option<PipelineState>,
    pub current_station: OptionDiff<Station>,
    pub current_track_tags: OptionDiff<TrackTags>,
    pub volume: Option<i32>,
    pub buffering: Option<u8>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub enum OutgoingMessage {
    PlayStateChanged(PlayerStateDiff),
}
