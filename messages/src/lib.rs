use std::time::Duration;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub const VOLUME_ZERO_DB: i32 = 100;
pub const VOLUME_MIN: i32 = 0;
pub const VOLUME_MAX: i32 = 120;

/// Commands from the user
#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub enum Command {
    SetChannel(String),
    PlayPause,
    PreviousItem,
    NextItem,
    SeekTo(Duration),
    VolumeUp,
    VolumeDown,
    SetVolume(i32),
    PlayUrl(String),
    Eject,
}

#[derive(Copy, Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
pub enum PipelineState {
    VoidPending,
    Null,
    Ready,
    Paused,
    Playing,
}

impl Default for PipelineState {
    fn default() -> Self {
        Self::VoidPending
    }
}

impl std::fmt::Display for PipelineState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(match self {
            Self::VoidPending => "VoidPending",
            Self::Null => "Null",
            Self::Ready => "Ready",
            Self::Paused => "Paused",
            Self::Playing => "Playing",
        })
    }
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
    pub organisation: Option<S>,
    pub artist: Option<S>,
    pub album: Option<S>,
    pub genre: Option<S>,
    pub image: Option<S>,
    pub comment: Option<S>,
}

impl<S: AsRef<str>> std::default::Default for TrackTags<S> {
    fn default() -> Self {
        Self {
            title: None,
            organisation: None,
            artist: None,
            album: None,
            genre: None,
            image: None,
            comment: None,
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
    pub current_track_index: Option<usize>,
    pub current_track_tags: OptionDiff<TrackTags<S>>,
    pub volume: Option<i32>,
    pub buffering: Option<u8>,
    pub track_duration: Option<Duration>,
    pub track_position: Option<Duration>,
}

#[derive(Copy, Clone, Debug, serde::Deserialize, serde::Serialize)]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct LogMessage<S: AsRef<str>> {
    pub level: LogLevel,
    pub message: S,
}

impl<S: AsRef<str>> LogMessage<S> {
    pub fn error(message: S) -> Self {
        Self {
            level: LogLevel::Error,
            message,
        }
    }

    pub fn warn(message: S) -> Self {
        Self {
            level: LogLevel::Warn,
            message,
        }
    }

    pub fn info(message: S) -> Self {
        Self {
            level: LogLevel::Info,
            message,
        }
    }

    pub fn debug(message: S) -> Self {
        Self {
            level: LogLevel::Debug,
            message,
        }
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub enum Event<Version: AsRef<str>, S: AsRef<str>, Tracklist: AsRef<[Track]>> {
    ProtocolVersion(Version),
    PlayerStateChanged(PlayerStateDiff<S, Tracklist>),
    LogMessage(LogMessage<S>),
}

impl<Version: AsRef<str>, S: AsRef<str>, Tracklist: AsRef<[Track]>>
    std::convert::From<PlayerStateDiff<S, Tracklist>> for Event<Version, S, Tracklist>
{
    fn from(diff: PlayerStateDiff<S, Tracklist>) -> Self {
        Self::PlayerStateChanged(diff)
    }
}

impl<Version: AsRef<str>, S: AsRef<str>, Tracklist: AsRef<[Track]>>
    std::convert::From<LogMessage<S>> for Event<Version, S, Tracklist>
{
    fn from(message: LogMessage<S>) -> Self {
        Self::LogMessage(message)
    }
}

pub fn protocol_version_message<S: AsRef<str>, TrackList: AsRef<[Track]>>(
) -> Event<&'static str, S, TrackList> {
    Event::ProtocolVersion(VERSION)
}
