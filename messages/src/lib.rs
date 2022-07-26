use std::{fmt, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};

pub use arcstr;
pub use arcstr::ArcStr;

mod encoding;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// When connecting over TCP, RRadio will begin by immediately sending the following header
/// Clients MUST verify that the header matches the header of the version of `rradio_messages` that they're linked to
pub const API_VERSION_HEADER: &str =
    concat!(env!("CARGO_PKG_NAME"), "_", env!("CARGO_PKG_VERSION"), "\n");

pub const API_VERSION_HEADER_LENGTH: usize = API_VERSION_HEADER.as_bytes().len();

/// The port to connect to for sending commands and receiving events
pub const API_PORT: u16 = 8002;

pub const VOLUME_ZERO_DB: i32 = 100;
pub const VOLUME_MIN: i32 = 0;
pub const VOLUME_MAX: i32 = 120;

#[derive(Debug, Deserialize, Serialize)]
pub struct SetPlaylistTrack {
    pub title: String,
    pub url: String,
}

/// Commands from the user
#[derive(Debug, Deserialize, Serialize)]
pub enum Command {
    SetChannel(StationIndex),
    PlayPause,
    SmartPreviousItem,
    PreviousItem,
    NextItem,
    NthItem(usize),
    SeekTo(Duration),
    SeekBackwards(Duration),
    SeekForwards(Duration),
    VolumeUp,
    VolumeDown,
    SetVolume(i32),
    SetPlaylist {
        title: String,
        tracks: Vec<SetPlaylistTrack>,
    },
    Eject,
    DebugPipeline,
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to encode Command: {0}")]
pub struct CommandEncodeError(#[source] postcard::Error);

#[derive(Debug, thiserror::Error)]
#[error("Failed to decode Command: {0}")]
pub struct CommandDecodeError(#[source] postcard::Error);

impl Command {
    /// Clear the buffer and encode the `Command` into it
    pub fn encode<'a>(&self, buffer: &'a mut Vec<u8>) -> Result<&'a [u8], CommandEncodeError> {
        encoding::encode_value(self, buffer).map_err(CommandEncodeError)
    }

    /// Decode a `Command` from the buffer
    pub fn decode(buffer: &mut [u8]) -> Result<Self, CommandDecodeError> {
        encoding::decode_value(buffer).map_err(CommandDecodeError)
    }
}

#[cfg(feature = "async")]
mod command_async {
    #[derive(Debug, thiserror::Error)]
    pub enum CommandStreamDecodeError {
        #[error("Failed to read Command")]
        IoError(#[from] std::io::Error),
        #[error(transparent)]
        DecodeError(#[from] super::CommandDecodeError),
    }

    impl From<postcard::Error> for CommandStreamDecodeError {
        fn from(err: postcard::Error) -> Self {
            Self::DecodeError(super::CommandDecodeError(err))
        }
    }

    #[derive(Debug, thiserror::Error)]
    pub enum CommandStreamEncodeError {
        #[error("Failed to send Command")]
        IoError(#[from] std::io::Error),
        #[error(transparent)]
        EncodeError(#[from] super::CommandEncodeError),
    }

    impl From<postcard::Error> for CommandStreamEncodeError {
        fn from(err: postcard::Error) -> Self {
            Self::EncodeError(super::CommandEncodeError(err))
        }
    }
}

#[cfg(feature = "async")]
use command_async::*;

#[cfg(feature = "async")]
impl Command {
    pub fn decode_from_stream<S: tokio::io::AsyncBufRead + Unpin>(
        stream: S,
    ) -> impl futures_util::Stream<Item = Result<Self, CommandStreamDecodeError>> {
        encoding::decode_from_stream(stream)
    }

    pub fn encode_to_stream<S: tokio::io::AsyncWrite + Unpin>(
        stream: S,
    ) -> impl futures_util::Sink<Self, Error = CommandStreamEncodeError> {
        encoding::encode_to_stream(stream)
    }
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

impl fmt::Display for PipelineState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Self::VoidPending => "VoidPending",
            Self::Null => "Null",
            Self::Ready => "Ready",
            Self::Paused => "Paused",
            Self::Playing => "Playing",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
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

impl From<SetPlaylistTrack> for Track {
    fn from(SetPlaylistTrack { title, url }: SetPlaylistTrack) -> Self {
        Self {
            title: Some(title.into()),
            album: None,
            artist: None,
            url: url.into(),
            is_notification: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
pub struct StationIndex(Box<str>);

impl StationIndex {
    pub fn new(index: Box<str>) -> Self {
        Self(index)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for StationIndex {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::borrow::Borrow<str> for StationIndex {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StationIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub enum StationType {
    UrlList,
    UPnP,
    Samba,
    CD,
    Usb,
}

impl fmt::Display for StationType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(match self {
            Self::UrlList => "URL List",
            Self::UPnP => "UPnP",
            Self::Samba => "Samba Server",
            Self::CD => "CD",
            Self::Usb => "USB",
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Station {
    pub index: Option<StationIndex>,
    pub source_type: StationType,
    pub title: Option<ArcStr>,
    pub tracks: Option<Arc<[Track]>>, // If None, the tracks haven't been loaded yet
}

/// The image tag of a track.
/// This wrapper is to avoid dumping to contents of an image to the terminal when debug printing a track tag.
#[derive(Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Image(ArcStr);

impl Image {
    pub fn new(mime_type: &str, image_data: &[u8]) -> Self {
        Self(format!("data:{};base64,{}", mime_type, base64::encode(image_data)).into())
    }
}

impl AsRef<[u8]> for Image {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl AsRef<str> for Image {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl std::ops::Deref for Image {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_ref()
    }
}

impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.0.hash(&mut hasher);

        write!(f, "Image {{ hash: {:016X} }}", hasher.finish())
    }
}

impl fmt::Display for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct TrackTags {
    pub title: Option<ArcStr>,
    pub organisation: Option<ArcStr>,
    pub artist: Option<ArcStr>,
    pub album: Option<ArcStr>,
    pub genre: Option<ArcStr>,
    pub image: Option<Image>,
    pub comment: Option<ArcStr>,
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

impl Default for PingTimes {
    fn default() -> Self {
        Self::None
    }
}

/// `PlayerStateDiff` records what fields have changed since the last diff was sent. If a field is `Some(_)`, then it has changed
#[derive(Debug, Deserialize, Serialize)]
pub struct PlayerStateDiff {
    pub pipeline_state: Option<PipelineState>,
    pub current_station: Option<Option<Station>>,
    pub pause_before_playing: Option<Option<Duration>>,
    pub current_track_index: Option<usize>,
    pub current_track_tags: Option<Option<TrackTags>>,
    pub volume: Option<i32>,
    pub buffering: Option<u8>,
    pub track_duration: Option<Option<Duration>>,
    pub track_position: Option<Option<Duration>>,
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
    #[error("UPnP Error: {0}")]
    UPnPError(ArcStr),
    #[error("Failed to read from stations directory {directory:?}: {err}")]
    StationsDirectoryIoError { directory: ArcStr, err: ArcStr },
    #[error("Station {index} not found in {directory}")]
    StationNotFound {
        index: StationIndex,
        directory: ArcStr,
    },
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
    PlayerStateChanged(PlayerStateDiff),
    LogMessage(LogMessage),
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to encode Event: {0}")]
pub struct EventEncodeError(#[source] postcard::Error);

#[derive(Debug, thiserror::Error)]
#[error("Failed to decode Event: {0}")]
pub struct EventDecodeError(#[source] postcard::Error);

impl Event {
    /// Clear the buffer and encode the `Event` into it
    pub fn encode<'a>(&self, buffer: &'a mut Vec<u8>) -> Result<&'a [u8], EventEncodeError> {
        encoding::encode_value(self, buffer).map_err(EventEncodeError)
    }

    /// Decode an `Event` from the buffer. Events are [COBS](https://en.wikipedia.org/wiki/Consistent_Overhead_Byte_Stuffing) encoded,
    /// and thus do not contain the value `0`, and are thus suffixed with a value of `0`
    ///
    /// # Example
    ///
    /// ```rust
    /// async fn read_next_event<S>(
    ///     stream: &mut S,
    ///     buffer: &mut Vec<u8>
    /// ) -> anyhow::Result<Option<rradio_messages::Event>>
    /// where
    ///     S: tokio::io::AsyncBufRead + Unpin,
    /// {
    ///     use anyhow::Context;
    ///     use tokio::io::AsyncBufReadExt;
    ///
    ///     buffer.clear();
    ///     let read_size = stream
    ///         .read_until(0, buffer)
    ///         .await
    ///         .context("Failed to read from stream")?;
    ///
    ///     if read_size == 0 {
    ///         return Ok(None);
    ///     }
    ///
    ///     let event = rradio_messages::Event::decode(buffer).context("Failed to decode Event")?;
    ///
    ///     Ok(Some(event))
    /// }
    /// ```
    pub fn decode(buffer: &mut [u8]) -> Result<Self, EventDecodeError> {
        encoding::decode_value(buffer).map_err(EventDecodeError)
    }
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

#[cfg(feature = "async")]
mod event_async {
    use std::fmt;

    struct DisplayHeader<'a>(&'a [u8]);

    impl<'a> fmt::Display for DisplayHeader<'a> {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            for &b in self.0.iter() {
                match b {
                    b'\n' => write!(f, "\\n")?,
                    b'\r' => write!(f, "\\r")?,
                    b'\t' => write!(f, "\\t")?,
                    b'\\' => write!(f, "\\\\")?,
                    b'\0' => write!(f, "\\0")?,
                    0x20..=0x7E => write!(f, "{}", b as char)?,
                    _ => write!(f, "\\x{:02x}", b)?,
                }
            }

            Ok(())
        }
    }

    #[derive(Debug, thiserror::Error)]
    pub enum BadRRadioHeader {
        #[error("Failed to read header")]
        FailedToReadHeader(#[from] std::io::Error),
        #[error("API version header does not match. Expected: {} Actual: {}", DisplayHeader(expected), DisplayHeader(&actual[..]))]
        HeaderMismatch {
            expected: &'static [u8],
            actual: [u8; super::API_VERSION_HEADER_LENGTH],
        },
    }

    pub(crate) async fn verify_rradio_header<S: tokio::io::AsyncRead + Unpin>(
        mut stream: S,
    ) -> Result<S, BadRRadioHeader> {
        use tokio::io::AsyncReadExt;

        let mut buffer = [0_u8; super::API_VERSION_HEADER_LENGTH];

        stream.read_exact(&mut buffer).await?;

        if super::API_VERSION_HEADER.as_bytes() == buffer {
            Ok(stream)
        } else {
            Err(BadRRadioHeader::HeaderMismatch {
                expected: super::API_VERSION_HEADER.as_bytes(),
                actual: buffer,
            })
        }
    }

    #[derive(Debug, thiserror::Error)]
    pub enum EventStreamDecodeError {
        #[error("Failed to read Event")]
        IoError(#[from] std::io::Error),
        #[error(transparent)]
        DecodeError(#[from] super::EventDecodeError),
    }

    impl From<postcard::Error> for EventStreamDecodeError {
        fn from(err: postcard::Error) -> Self {
            Self::DecodeError(super::EventDecodeError(err))
        }
    }

    #[derive(Debug, thiserror::Error)]
    pub enum EventStreamEncodeError {
        #[error("Failed to send Event")]
        IoError(#[from] std::io::Error),
        #[error(transparent)]
        EncodeError(#[from] super::EventEncodeError),
    }

    impl From<postcard::Error> for EventStreamEncodeError {
        fn from(err: postcard::Error) -> Self {
            Self::EncodeError(super::EventEncodeError(err))
        }
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn print_incorrect_header() {
            use std::io::Write;
            let mut actual = [0; super::super::API_VERSION_HEADER_LENGTH];

            for (b, index) in actual.iter_mut().zip(0..) {
                *b = index;
            }

            write!(&mut actual[..], "BAD_HEADER\n\r\t\\\0").unwrap();

            let err = super::BadRRadioHeader::HeaderMismatch {
                expected: super::super::API_VERSION_HEADER.as_bytes(),
                actual,
            };

            println!("{}", err);
        }
    }
}

#[cfg(feature = "async")]
use event_async::*;

#[cfg(feature = "async")]
impl Event {
    pub async fn decode_from_stream<S: tokio::io::AsyncBufRead + Unpin>(
        stream: S,
    ) -> Result<
        impl futures_util::Stream<Item = Result<Self, EventStreamDecodeError>>,
        BadRRadioHeader,
    > {
        verify_rradio_header(stream)
            .await
            .map(encoding::decode_from_stream)
    }

    pub fn encode_to_stream<S: tokio::io::AsyncWrite + Unpin>(
        stream: S,
    ) -> impl futures_util::Sink<Self, Error = EventStreamEncodeError> {
        encoding::encode_to_stream(stream)
    }
}
