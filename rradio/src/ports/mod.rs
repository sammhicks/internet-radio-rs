//! Ports are the access point for external programs to interact with ``RRadio``.
//! Through ports a client can listen for [Events](rradio_messages::Event) and submit [Commands](rradio_messages::Command).

use std::sync::Arc;

use anyhow::Context;
use futures_util::{FutureExt, Sink, StreamExt};
use rradio_messages::PlayerStateDiff;

use crate::{pipeline::PlayerState, task::ShutdownSignal};

mod stream;

pub mod tcp;
pub mod tcp_binary;
pub mod tcp_text;

#[cfg(feature = "web")]
pub mod web;

fn player_state_to_diff(state: &PlayerState) -> PlayerStateDiff {
    PlayerStateDiff {
        pipeline_state: Some(state.pipeline_state),
        current_station: Some(state.current_station.as_ref().clone()),
        pause_before_playing: Some(state.pause_before_playing),
        current_track_index: Some(state.current_track_index),
        current_track_tags: Some(state.current_track_tags.as_ref().clone()),
        is_muted: Some(state.is_muted),
        volume: Some(state.volume),
        buffering: Some(state.buffering),
        track_duration: Some(state.track_duration),
        track_position: Some(state.track_position),
        ping_times: Some(state.ping_times.clone()),
        latest_error: Some(state.latest_error.as_ref().clone()),
    }
}

fn diff_player_state(a: &PlayerState, b: &PlayerState) -> Option<PlayerStateDiff> {
    let mut any_some = false;
    let diff = PlayerStateDiff {
        pipeline_state: diff_value(&a.pipeline_state, &b.pipeline_state, &mut any_some),
        current_station: diff_arc_with_clone(&a.current_station, &b.current_station, &mut any_some),
        pause_before_playing: diff_value(
            &a.pause_before_playing,
            &b.pause_before_playing,
            &mut any_some,
        ),
        current_track_index: diff_value(
            &a.current_track_index,
            &b.current_track_index,
            &mut any_some,
        ),
        current_track_tags: diff_arc_with_clone(
            &a.current_track_tags,
            &b.current_track_tags,
            &mut any_some,
        ),
        is_muted: diff_value(&a.is_muted, &b.is_muted, &mut any_some),
        volume: diff_value(&a.volume, &b.volume, &mut any_some),
        buffering: diff_value(&a.buffering, &b.buffering, &mut any_some),
        track_duration: diff_value(&a.track_duration, &b.track_duration, &mut any_some),
        track_position: diff_value(&a.track_position, &b.track_position, &mut any_some),
        ping_times: diff_value(&a.ping_times, &b.ping_times, &mut any_some),
        latest_error: diff_arc_with_clone(&a.latest_error, &b.latest_error, &mut any_some),
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

#[derive(Debug, Clone)]
struct CommandSink(pub tokio::sync::mpsc::UnboundedSender<rradio_messages::Command>);

impl Sink<rradio_messages::Command> for CommandSink {
    type Error = anyhow::Error;

    fn poll_ready(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn start_send(
        self: std::pin::Pin<&mut Self>,
        item: rradio_messages::Command,
    ) -> Result<(), Self::Error> {
        self.0.send(item).context("Failed to send command")
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}

pub struct NoShutdownSignal;

/// The channel endpoints which ports use to communicate with the pipeline
/// The name "partial" is because `shutdown_signal` is initially of type `()` and is replaced with the actual shutdown signal, of type [`ShutdownSignal`]
#[derive(Clone)]
pub struct PartialPortChannels<SS> {
    pub commands_tx: tokio::sync::mpsc::UnboundedSender<rradio_messages::Command>,
    pub player_state_rx: tokio::sync::watch::Receiver<PlayerState>,
    pub shutdown_signal: SS,
}

/// [`PartialPortChannels`] with a [`ShutdownSignal`] becomes an entire `PortChannels`
pub type PortChannels = PartialPortChannels<futures_util::future::Shared<ShutdownSignal>>;

impl PartialPortChannels<NoShutdownSignal> {
    pub fn with_shutdown_signal(self, shutdown_signal: ShutdownSignal) -> PortChannels {
        PortChannels {
            commands_tx: self.commands_tx,
            player_state_rx: self.player_state_rx,
            shutdown_signal: shutdown_signal.shared(),
        }
    }
}

impl PortChannels {
    pub fn event_stream(&self) -> impl futures_util::Stream<Item = rradio_messages::Event> {
        let player_state_rx = self.player_state_rx.clone();
        let current_state = player_state_rx.borrow().clone();
        futures_util::stream::once(futures_util::future::ready(
            rradio_messages::Event::PlayerStateChanged(player_state_to_diff(&current_state)),
        )) // Set the current state as an "everything has changed" diff
        .chain(
            // Whenever the player state changed, diff the current state with the new state and if the diff isn't empty, send it
            futures_util::stream::unfold(
                (player_state_rx, current_state),
                |(mut player_state_rx, current_state)| async move {
                    loop {
                        player_state_rx.changed().await.ok()?;
                        let new_state = player_state_rx.borrow().clone();
                        match diff_player_state(&current_state, &new_state) {
                            Some(diff) => {
                                return Some((
                                    rradio_messages::Event::PlayerStateChanged(diff),
                                    (player_state_rx, new_state),
                                ))
                            }
                            None => continue,
                        }
                    }
                },
            ),
        )
        .take_until(self.shutdown_signal.clone())
    }
}
