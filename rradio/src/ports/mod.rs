pub mod tcp_text;

use std::sync::Arc;

use crate::pipeline::PlayerState;
use rradio_messages::PlayerStateDiff;

fn state_to_diff(state: &PlayerState) -> PlayerStateDiff {
    PlayerStateDiff {
        pipeline_state: Some(state.pipeline_state),
        current_station: state.current_station.as_ref().clone().into(),
        current_track_tags: state.current_track_tags.as_ref().clone().into(),
        volume: Some(state.volume),
        buffering: Some(state.buffering),
    }
}

fn diff_player_state(a: &PlayerState, b: &PlayerState) -> PlayerStateDiff {
    PlayerStateDiff {
        pipeline_state: diff_value(&a.pipeline_state, &b.pipeline_state),
        current_station: diff_arc_with_clone(&a.current_station, &b.current_station).into(),
        current_track_tags: diff_arc_with_clone(&a.current_track_tags, &b.current_track_tags)
            .into(),
        volume: diff_value(&a.volume, &b.volume),
        buffering: diff_value(&a.buffering, &b.buffering),
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
