use std::{fmt::Debug, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};

pub use arcstr;
pub use arcstr::ArcStr;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

pub type MsgPackBufferLength = u32;

pub const VOLUME_ZERO_DB: i32 = 100;
pub const VOLUME_MIN: i32 = 0;
pub const VOLUME_MAX: i32 = 120;

/// Commands from the user
#[derive(Debug, Deserialize, Serialize)]
pub enum Command {
    SetChannel(String),
    PlayPause,
    SmartPreviousItem,
    PreviousItem,
    NextItem,
    SeekTo(Duration),
    SeekBackwards(Duration),
    SeekForwards(Duration),
    VolumeUp,
    VolumeDown,
    SetVolume(i32),
    PlayUrl(String),
    Eject,
    DebugPipeline,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
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

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Track {
    pub title: Option<ArcStr>,
    pub album: Option<ArcStr>,
    pub artist: Option<ArcStr>,
    pub url: ArcStr,
    pub is_notification: bool,
}

impl Track {
    pub fn url(url: ArcStr) -> Self {
        Self {
            title: None,
            album: None,
            artist: None,
            url,
            is_notification: false,
        }
    }

    pub fn notification(url: ArcStr) -> Self {
        Self {
            title: None,
            album: None,
            artist: None,
            url,
            is_notification: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum StationType {
    UrlList,
    Samba,
    CD,
    Usb,
}

impl std::fmt::Display for StationType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.pad(match self {
            Self::UrlList => "URL List",
            Self::Samba => "Samba Server",
            Self::CD => "CD",
            Self::Usb => "USB",
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Station {
    pub index: Option<ArcStr>,
    pub source_type: StationType,
    pub title: Option<ArcStr>,
    pub tracks: Arc<[Track]>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct TrackTags {
    pub title: Option<ArcStr>,
    pub organisation: Option<ArcStr>,
    pub artist: Option<ArcStr>,
    pub album: Option<ArcStr>,
    pub genre: Option<ArcStr>,
    pub image: Option<ArcStr>,
    pub comment: Option<ArcStr>,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum OptionDiff<T> {
    NoChange,
    ChangedToNone,
    ChangedToSome(T),
}

impl<T> OptionDiff<T> {
    pub fn has_changed(&self) -> bool {
        !self.has_not_changed()
    }

    pub fn has_not_changed(&self) -> bool {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error, Deserialize, Serialize)]
pub enum PingError {
    /// Failed to resolve hostname into IP address
    #[error("DNS Failure")]
    Dns,
    /// OS raised error when sending ICMP message
    #[error("Failed to send ICMP Message")]
    FailedToSendICMP,
    /// OS raised error when receiving ICMP message
    #[error("Failed to recieve ICMP Message")]
    FailedToRecieveICMP,
    /// Timeout before receiving ICMP message
    #[error("Timeout on ICMP Message")]
    Timeout,
    /// Ping response reported as "Destination Unreachable"
    #[error("Destination Unreachable")]
    DestinationUnreachable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum PingTarget {
    Gateway,
    Remote,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum PingTimes {
    None,
    BadUrl,
    Gateway(Result<Duration, PingError>),
    GatewayAndRemote {
        gateway_ping: Duration,
        remote_ping: Result<Duration, PingError>,
        latest: PingTarget,
    },
    FinishedPingingRemote {
        gateway_ping: Duration,
    },
}

#[derive(Debug, Deserialize, Serialize)]
pub struct PlayerStateDiff {
    pub pipeline_state: Option<PipelineState>,
    pub current_station: OptionDiff<Station>,
    pub pause_before_playing: OptionDiff<Duration>,
    pub current_track_index: Option<usize>,
    pub current_track_tags: OptionDiff<TrackTags>,
    pub volume: Option<i32>,
    pub buffering: Option<u8>,
    pub track_duration: OptionDiff<Duration>,
    pub track_position: OptionDiff<Duration>,
    pub ping_times: Option<PingTimes>,
}

#[derive(Clone, Debug, Deserialize, Serialize, thiserror::Error)]
#[error("Pipeline Error: {0}")]
pub struct PipelineError(pub ArcStr);

#[derive(Clone, Debug, Deserialize, Serialize, thiserror::Error)]
pub enum CdError {
    #[error("CD support is not enabled")]
    CdNotEnabled,
    #[error("Failed to open CD device: {0}")]
    FailedToOpenDevice(ArcStr),
    #[error("ioctl Error: {0}")]
    IoCtlError(ArcStr),
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

#[derive(Clone, Debug, Deserialize, Serialize, thiserror::Error)]
pub enum EjectError {
    #[error("Failed to open CD device")]
    FailedToOpenDevice,
    #[error("Failed to eject CD")]
    FailedToEjectDevice,
}

#[derive(Clone, Debug, Deserialize, Serialize, thiserror::Error)]
pub enum MountError {
    #[error("USB support is not enabled")]
    UsbNotEnabled,
    #[error("Samba support is not enabled")]
    SambaNotEnabled,
    #[error("Not found")]
    NotFound,
    #[error("Failed to create temporary directory: {0}")]
    CouldNotCreateTemporaryDirectory(ArcStr),
    #[error("Failed to mount {device}: {err}")]
    CouldNotMountDevice { device: ArcStr, err: ArcStr },
    #[error("Error finding tracks: {0}")]
    ErrorFindingTracks(ArcStr),
    #[error("Tracks not found")]
    TracksNotFound,
}

#[derive(Clone, Debug, Deserialize, Serialize, thiserror::Error)]
pub enum StationError {
    #[error("CD Error: {0}")]
    CdError(#[from] CdError),
    #[error("Mount Error: {0}")]
    MountError(#[from] MountError),
    #[error("Failed to read from stations directory {directory:?}: {err}")]
    StationsDirectoryIoError { directory: ArcStr, err: ArcStr },
    #[error("Station {index} not found in {directory}")]
    StationNotFound { index: ArcStr, directory: ArcStr },
    #[error("Bad Station File: {0}")]
    BadStationFile(ArcStr),
}

#[derive(Clone, Debug, Deserialize, Serialize, thiserror::Error)]
#[error("Tag Error: {0}")]
pub struct TagError(pub ArcStr);

#[derive(Clone, Debug, Deserialize, Serialize, thiserror::Error)]
pub enum Error {
    #[error("No Playlist")]
    NoPlaylist,
    #[error("Invalid track index: {0}")]
    InvalidTrackIndex(usize),
    #[error(transparent)]
    PipelineError(#[from] PipelineError),
    #[error("Station Error: {0}")]
    StationError(#[from] StationError),
    #[error(transparent)]
    EjectError(#[from] EjectError),
    #[error(transparent)]
    TagError(#[from] TagError),
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum LogMessage {
    Error(Error),
}

impl std::convert::From<Error> for LogMessage {
    fn from(error: Error) -> Self {
        Self::Error(error)
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Deserialize, Serialize)]
pub enum Event {
    ProtocolVersion(ArcStr),
    PlayerStateChanged(PlayerStateDiff),
    LogMessage(LogMessage),
}

impl std::convert::From<PlayerStateDiff> for Event {
    fn from(diff: PlayerStateDiff) -> Self {
        Self::PlayerStateChanged(diff)
    }
}

impl std::convert::From<LogMessage> for Event {
    fn from(message: LogMessage) -> Self {
        Self::LogMessage(message)
    }
}

pub fn protocol_version_message() -> Event {
    Event::ProtocolVersion(VERSION.into())
}
