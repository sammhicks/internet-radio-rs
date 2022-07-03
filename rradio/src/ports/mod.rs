//! Ports are the access point for external programs to interact with ``RRadio``.
//! Through ports a client can listen for [Events](rradio_messages::Event) and submit [Commands](rradio_messages::Command).

use std::sync::Arc;

use rradio_messages::{LogMessage, PlayerStateDiff};

use crate::{pipeline::PlayerState, task::ShutdownSignal};

pub mod tcp;
pub mod tcp_msgpack;
pub mod tcp_text;

#[cfg(feature = "web")]
pub mod web;

pub type BroadcastEvent = rradio_messages::Event;

fn player_state_to_diff(state: &PlayerState) -> PlayerStateDiff {
    PlayerStateDiff {
        pipeline_state: Some(state.pipeline_state),
        current_station: state.current_station.as_ref().clone().into(),
        pause_before_playing: state.pause_before_playing.into(),
        current_track_index: Some(state.current_track_index),
        current_track_tags: state.current_track_tags.as_ref().clone().into(),
        volume: Some(state.volume),
        buffering: Some(state.buffering),
        track_duration: state.track_duration.into(),
        track_position: state.track_position.into(),
        ping_times: state.ping_times.clone().into(),
    }
}

fn diff_player_state(a: &PlayerState, b: &PlayerState) -> Option<PlayerStateDiff> {
    let mut any_some = false;
    let diff = PlayerStateDiff {
        pipeline_state: diff_value(&a.pipeline_state, &b.pipeline_state, &mut any_some),
        current_station: diff_arc_with_clone(&a.current_station, &b.current_station, &mut any_some)
            .into(),
        pause_before_playing: diff_value(
            &a.pause_before_playing,
            &b.pause_before_playing,
            &mut any_some,
        )
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
        ping_times: diff_value(&a.ping_times, &b.ping_times, &mut any_some),
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

#[allow(clippy::large_enum_variant)]
enum Event {
    StateUpdate(PlayerStateDiff),
    LogMessage(LogMessage),
}

impl Event {
    fn into_broadcast_event(self) -> BroadcastEvent {
        match self {
            Event::StateUpdate(diff) => BroadcastEvent::PlayerStateChanged(diff),
            Event::LogMessage(log_message) => BroadcastEvent::LogMessage(log_message),
        }
    }
}

fn event_stream(
    player_state_rx: tokio::sync::watch::Receiver<PlayerState>,
    log_messages_rx: tokio::sync::broadcast::Receiver<LogMessage>,
) -> impl futures::Stream<Item = Event> {
    use futures::StreamExt;

    let state_diff_stream = {
        let current_state = player_state_rx.borrow().clone();
        futures::stream::once(futures::future::ready(Event::StateUpdate(
            player_state_to_diff(&current_state),
        ))) // Set the current state as an "everything has changed" diff
        .chain(
            // Whenever the player state changed, diff the current state with the new state and if the diff isn't empty, send it
            futures::stream::unfold(
                (player_state_rx, current_state),
                |(mut player_state_rx, current_state)| async move {
                    loop {
                        player_state_rx.changed().await.ok()?;
                        let new_state = player_state_rx.borrow().clone();
                        match diff_player_state(&current_state, &new_state) {
                            Some(diff) => {
                                return Some((
                                    Event::StateUpdate(diff),
                                    (player_state_rx, new_state),
                                ))
                            }
                            None => continue,
                        }
                    }
                },
            ),
        )
    };

    let log_stream = futures::stream::unfold(log_messages_rx, |mut log_messages_rx| async {
        loop {
            return match log_messages_rx.recv().await {
                Ok(message) => Some((Event::LogMessage(message), log_messages_rx)),
                Err(tokio::sync::broadcast::error::RecvError::Closed) => None,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue, // Ignore log message loss
            };
        }
    });

    futures::stream::select(state_diff_stream, log_stream)
}

/// The channel endpoints which ports use to communicate with the pipeline
/// The name "partial" is because `shutdown_signal` is initially of type `()` and is replaced with the actual shutdown signal, of type [`ShutdownSignal`]
#[derive(Clone)]
pub struct PartialPortChannels<SS> {
    pub commands_tx: tokio::sync::mpsc::UnboundedSender<rradio_messages::Command>,
    pub player_state_rx: tokio::sync::watch::Receiver<PlayerState>,
    pub log_message_source: crate::pipeline::LogMessageSource,
    pub shutdown_signal: SS,
}

/// [`PartialPortChannels`] with a [`ShutdownSignal`] becomes an entire `PortChannels`
pub type PortChannels = PartialPortChannels<ShutdownSignal>;

impl<SS> PartialPortChannels<SS> {
    pub fn with_shutdown_signal(self, shutdown_signal: ShutdownSignal) -> PortChannels {
        PortChannels {
            commands_tx: self.commands_tx,
            player_state_rx: self.player_state_rx,
            log_message_source: self.log_message_source,
            shutdown_signal,
        }
    }
}
