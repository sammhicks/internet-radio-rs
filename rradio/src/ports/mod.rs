mod connection_stream;
pub mod tcp;
pub mod tcp_msgpack;
pub mod tcp_text;

#[cfg(feature = "web")]
pub mod web;

use std::sync::Arc;

use rradio_messages::{PlayerStateDiff, Track};

use crate::{atomic_string::AtomicString, pipeline::PlayerState, task::ShutdownSignal};

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
        track_duration: state.track_duration.into(),
        track_position: state.track_position.into(),
    }
}

fn diff_player_state(
    a: &PlayerState,
    b: &PlayerState,
) -> Option<PlayerStateDiff<AtomicString, TrackList>> {
    let mut any_some = false;
    let diff = PlayerStateDiff {
        pipeline_state: diff_value(&a.pipeline_state, &b.pipeline_state, &mut any_some),
        current_station: diff_arc_with_clone(&a.current_station, &b.current_station, &mut any_some)
            .into(),
        current_track_index: diff_value(
            &a.current_track_index,
            &b.current_track_index,
            &mut any_some,
        ),
        current_track_tags: diff_arc_with_clone(
            &a.current_track_tags,
            &b.current_track_tags,
            &mut any_some,
        )
        .into(),
        volume: diff_value(&a.volume, &b.volume, &mut any_some),
        buffering: diff_value(&a.buffering, &b.buffering, &mut any_some),
        track_duration: diff_value(&a.track_duration, &b.track_duration, &mut any_some).into(),
        track_position: diff_value(&a.track_position, &b.track_position, &mut any_some).into(),
    };
    if any_some {
        Some(diff)
    } else {
        None
    }
}

fn diff_value<T: Clone + std::cmp::PartialEq>(a: &T, b: &T, any_some: &mut bool) -> Option<T> {
    if a == b {
        None
    } else {
        *any_some = true;
        Some(b.clone())
    }
}

fn diff_arc_with_clone<T: Clone>(a: &Arc<T>, b: &Arc<T>, any_some: &mut bool) -> Option<T> {
    if Arc::ptr_eq(a, b) {
        None
    } else {
        *any_some = true;
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

impl<SS> PartialPortChannels<SS> {
    pub fn with_shutdown_signal(self, shutdown_signal: ShutdownSignal) -> PortChannels {
        PortChannels {
            commands: self.commands,
            player_state: self.player_state,
            log_message_source: self.log_message_source,
            shutdown_signal,
        }
    }
}
