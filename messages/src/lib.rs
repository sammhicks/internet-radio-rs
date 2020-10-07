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

#[derive(Debug, PartialEq, serde::Deserialize, serde::Serialize)]
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
pub struct Station<S, TrackList>
where
    S: AsRef<str>,
    TrackList: AsRef<[Track]>,
{
    pub index: Option<S>,
    pub title: Option<S>,
    pub tracks: TrackList,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub struct TrackTags<S: AsRef<str>> {
    pub title: Option<S>,
    pub artist: Option<S>,
    pub album: Option<S>,
    pub genre: Option<S>,
    pub image: Option<S>,
}

impl<S: AsRef<str>> std::default::Default for TrackTags<S> {
    fn default() -> Self {
        Self {
            title: None,
            artist: None,
            album: None,
            genre: None,
            image: None,
        }
    }
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
pub struct PlayerStateDiff<S: AsRef<str>, TrackList: AsRef<[Track]>> {
    pub pipeline_state: Option<PipelineState>,
    pub current_station: OptionDiff<Station<S, TrackList>>,
    pub current_track_tags: OptionDiff<TrackTags<S>>,
    pub volume: Option<i32>,
    pub buffering: Option<u8>,
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub enum OutgoingMessage<S: AsRef<str>, T: AsRef<[Track]>> {
    PlayerStateChanged(PlayerStateDiff<S, T>),
}
