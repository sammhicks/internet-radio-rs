mod connection_stream;
mod shutdown;
pub mod tcp;
pub mod tcp_msgpack;
pub mod tcp_text;
mod wait_group;

#[cfg(feature = "web")]
pub mod web;

pub use shutdown::Signal as ShutdownSignal;
pub use wait_group::{Handle as WaitGroupHandle, WaitGroup};

use std::sync::Arc;

use crate::{atomic_string::AtomicString, pipeline::PlayerState};
use rradio_messages::{PlayerStateDiff, Track};

type TrackList = Arc<[Track]>;
pub type BroadcastEvent = rradio_messages::Event<&'static str, AtomicString, TrackList>;

fn player_state_to_diff(state: &PlayerState) -> PlayerStateDiff<AtomicString, TrackList> {
    PlayerStateDiff {
        pipeline_state: Some(state.pipeline_state),
        current_station: state.current_station.as_ref().clone().into(),
        current_track_index: Some(state.current_track_index),
        current_track_tags: state.current_track_tags.as_ref().clone().into(),
        volume: Some(state.volume),
        buffering: Some(state.buffering),
        track_duration: None,
        track_position: None,
    }
}

fn diff_player_state(a: &PlayerState, b: &PlayerState) -> PlayerStateDiff<AtomicString, TrackList> {
    PlayerStateDiff {
        pipeline_state: diff_value(&a.pipeline_state, &b.pipeline_state),
        current_station: diff_arc_with_clone(&a.current_station, &b.current_station).into(),
        current_track_index: diff_value(&a.current_track_index, &b.current_track_index),
        current_track_tags: diff_arc_with_clone(&a.current_track_tags, &b.current_track_tags)
            .into(),
        volume: diff_value(&a.volume, &b.volume),
        buffering: diff_value(&a.buffering, &b.buffering),
        track_duration: None,
        track_position: None,
    }
}

fn diff_value<T: Clone + std::cmp::PartialEq>(a: &T, b: &T) -> Option<T> {
    if a == b {
        None
    } else {
        Some(b.clone())
    }
}

fn diff_arc_with_clone<T: Clone>(a: &Arc<T>, b: &Arc<T>) -> Option<T> {
    if Arc::ptr_eq(a, b) {
        None
    } else {
        Some(b.as_ref().clone())
    }
}

enum Event {
    StateUpdate(PlayerState),
    LogMessage(rradio_messages::LogMessage<AtomicString>),
}

#[derive(Clone)]
pub struct PartialPortChannels<SS> {
    pub commands: tokio::sync::mpsc::UnboundedSender<rradio_messages::Command>,
    pub player_state: tokio::sync::watch::Receiver<PlayerState>,
    pub log_message_source: crate::pipeline::LogMessageSource,
    pub shutdown_signal: SS,
}

pub type PortChannels = PartialPortChannels<ShutdownSignal>;

impl<SS1> PartialPortChannels<SS1> {
    pub fn with_shutdown_signal(self, shutdown_signal: ShutdownSignal) -> PortChannels {
        PortChannels {
            commands: self.commands,
            player_state: self.player_state,
            log_message_source: self.log_message_source,
            shutdown_signal,
        }
    }
}
