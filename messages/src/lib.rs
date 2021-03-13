use std::{fmt::Debug, time::Duration};

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
    DebugPipeline,
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
    pub album: Option<String>,
    pub artist: Option<String>,
    pub url: String,
    pub is_notification: bool,
}

impl Track {
    pub fn url(url: String) -> Self {
        Self {
            title: None,
            album: None,
            artist: None,
            url,
            is_notification: false,
        }
    }

    pub fn notification(url: String) -> Self {
        Self {
            title: None,
            album: None,
            artist: None,
            url,
            is_notification: true,
        }
    }
}

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
pub enum StationType {
    UrlList,
    Samba,
    CD,
    USB,
}

impl std::fmt::Display for StationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(match self {
            Self::UrlList => "URL List",
            Self::Samba => "Samba Server",
            Self::CD => "CD",
            Self::USB => "USB",
        })
    }
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct Station<S, TrackList>
where
    S: AsRef<str>,
    TrackList: AsRef<[Track]>,
{
    pub index: Option<S>,
    pub source_type: StationType,
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

#[derive(Clone, Copy, Debug, serde::Deserialize, serde::Serialize)]
pub enum OptionDiff<T> {
    NoChange,
    ChangedToNone,
    ChangedToSome(T),
}

impl<T> OptionDiff<T> {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::NoChange)
    }

    pub fn into_option(self) -> Option<Option<T>> {
        match self {
            Self::NoChange => None,
            Self::ChangedToNone => Some(None),
            Self::ChangedToSome(t) => Some(Some(t)),
        }
    }
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

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum RemotePingErrorKind {
    Dns,
    Ping,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub struct RemotePingError<S> {
    pub kind: RemotePingErrorKind,
    pub message: S,
}

impl<S> std::cmp::PartialEq for RemotePingError<S> {
    fn eq(&self, other: &Self) -> bool {
        self.kind == other.kind
    }
}

impl<S> std::cmp::Eq for RemotePingError<S> {}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub enum PingTimes<S: AsRef<str>> {
    None,
    BadUrl {
        url: S,
        error_message: S,
    },
    Gateway(Result<Duration, S>),
    GatewayAndRemote {
        gateway_ping: Duration,
        remote_ping: Result<Duration, RemotePingError<S>>,
    },
    FinishedPingingRemote {
        gateway_ping: Duration,
    },
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub struct PlayerStateDiff<S: AsRef<str>, TrackList: AsRef<[Track]>> {
    pub pipeline_state: Option<PipelineState>,
    pub current_station: OptionDiff<Station<S, TrackList>>,
    pub pause_before_playing: OptionDiff<Duration>,
    pub current_track_index: Option<usize>,
    pub current_track_tags: OptionDiff<TrackTags<S>>,
    pub volume: Option<i32>,
    pub buffering: Option<u8>,
    pub track_duration: OptionDiff<Duration>,
    pub track_position: OptionDiff<Duration>,
    pub ping_times: Option<PingTimes<S>>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, thiserror::Error)]
#[error("Pipeline Error: {}", .0.as_ref())]
pub struct PipelineError<S: AsRef<str> + Debug>(pub S);

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, thiserror::Error)]
pub enum CdError<S: AsRef<str> + Debug> {
    #[error("CD support is not enabled")]
    CdNotEnabled,
    #[error("Failed to open CD device: {}", .0.as_ref())]
    FailedToOpenDevice(S),
    #[error("ioctl Error: {}", .0.as_ref())]
    IoCtlError(S),
    #[error("No CD info")]
    NoCdInfo,
    #[error("No CD")]
    NoCd,
    #[error("CD tray is open")]
    CdTrayIsOpen,
    #[error("CD tray is not ready")]
    CdTrayIsNotReady,
    #[error("CD is CDS_DATA_1")]
    CdIsData1,
    #[error("CD is CDS_DATA_2")]
    CdIsData2,
    #[error("CD is CDS_XA_2_1")]
    CdIsXA21,
    #[error("CD is CDS_XA_2_2")]
    CdIsXA22,
    #[error("Unknown Drive Status: {0}")]
    UnknownDriveStatus(isize),
    #[error("Unknown Disc Status: {0}")]
    UnknownDiscStatus(isize),
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, thiserror::Error)]
pub enum MountError<S: AsRef<str> + Debug> {
    #[error("USB support is not enabled")]
    UsbNotEnabled,
    #[error("Samba support is not enabled")]
    SambaNotEnabled,
    #[error("Not found")]
    NotFound,
    #[error("Failed to create temporary directory: {}", .0.as_ref())]
    CouldNotCreateTemporaryDirectory(S),
    #[error("Failed to mount {}: {}", .device.as_ref(), .err.as_ref())]
    CouldNotMountDevice { device: S, err: S },
    #[error("Error finding tracks: {}", .0.as_ref())]
    ErrorFindingTracks(S),
    #[error("Tracks not found")]
    TracksNotFound,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, thiserror::Error)]
pub enum StationError<S: AsRef<str> + Debug + 'static> {
    #[error("CD Error: {0}")]
    CdError(#[from] CdError<S>),
    #[error("Mount Error: {0}")]
    MountError(#[from] MountError<S>),
    #[error("Failed to read from stations directory {:?}: {}", directory.as_ref(), err.as_ref())]
    StationsDirectoryIoError { directory: S, err: S },
    #[error("Station {} not found in {}", index.as_ref(), directory.as_ref())]
    StationNotFound { index: S, directory: S },
    #[error("Bad Station File: {}", .0.as_ref())]
    BadStationFile(S),
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, thiserror::Error)]
#[error("Tag Error: {}", .0.as_ref())]
pub struct TagError<S: AsRef<str> + Debug>(pub S);

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize, thiserror::Error)]
pub enum Error<S: AsRef<str> + Debug + 'static> {
    #[error("No Playlist")]
    NoPlaylist,
    #[error("Invalid track index: {0}")]
    InvalidTrackIndex(usize),
    #[error(transparent)]
    PipelineError(#[from] PipelineError<S>),
    #[error("Station Error: {0}")]
    StationError(#[from] StationError<S>),
    #[error(transparent)]
    TagError(#[from] TagError<S>),
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
pub enum LogMessage<S: AsRef<str> + Debug + 'static> {
    Error(Error<S>),
}

impl<S: AsRef<str> + Debug> std::convert::From<Error<S>> for LogMessage<S> {
    fn from(error: Error<S>) -> Self {
        Self::Error(error)
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize)]
pub enum Event<Version: AsRef<str>, S: AsRef<str> + Debug + 'static, Tracklist: AsRef<[Track]>> {
    ProtocolVersion(Version),
    PlayerStateChanged(PlayerStateDiff<S, Tracklist>),
    LogMessage(LogMessage<S>),
}

impl<Version: AsRef<str>, S: AsRef<str> + Debug, Tracklist: AsRef<[Track]>>
    std::convert::From<PlayerStateDiff<S, Tracklist>> for Event<Version, S, Tracklist>
{
    fn from(diff: PlayerStateDiff<S, Tracklist>) -> Self {
        Self::PlayerStateChanged(diff)
    }
}

impl<Version: AsRef<str>, S: AsRef<str> + Debug, Tracklist: AsRef<[Track]>>
    std::convert::From<LogMessage<S>> for Event<Version, S, Tracklist>
{
    fn from(message: LogMessage<S>) -> Self {
        Self::LogMessage(message)
    }
}

pub fn protocol_version_message<S: AsRef<str> + Debug, TrackList: AsRef<[Track]>>(
) -> Event<&'static str, S, TrackList> {
    Event::ProtocolVersion(VERSION)
}
