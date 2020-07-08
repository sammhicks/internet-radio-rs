#![macro_use]

use tokio::sync::{mpsc, watch};
use warp::{
    reject::Reject,
    reply::{self, Reply},
    Filter,
};

macro_rules! check_for_arc_change {
    ($messages:ident, $current_state:ident, $new_state:ident, $field:ident) => {
        if $current_state.as_ref().map_or(true, |state| {
            !std::sync::Arc::ptr_eq(&state.$field, &$new_state.$field)
        }) {
            $messages.push(
                (
                    warp::sse::event(std::stringify!($field)),
                    warp::sse::data($new_state.$field.to_string()),
                )
                    .boxed(),
            );
        }
    };
}

macro_rules! check_for_json_arc_change {
    ($messages:ident, $current_state:ident, $new_state:ident, $field:ident) => {
        if $current_state.as_ref().map_or(true, |state| {
            !std::sync::Arc::ptr_eq(&state.$field, &$new_state.$field)
        }) {
            $messages.push(
                (
                    warp::sse::event(std::stringify!($field)),
                    warp::sse::json($new_state.$field.clone()),
                )
                    .boxed(),
            );
        }
    };
}

macro_rules! check_for_change {
    ($messages:ident, $current_state:ident, $new_state:ident, $field:ident) => {
        if Some(&$new_state.$field) != $current_state.as_ref().map(|state| &state.$field) {
            $messages.push(
                (
                    warp::sse::event(std::stringify!($field)),
                    warp::sse::data($new_state.$field.to_string()),
                )
                    .boxed(),
            );
        }
    };
}

use crate::message::{Command, PlayerState};

type Commands = mpsc::UnboundedSender<Command>;

struct SendError(mpsc::error::SendError<Command>);

impl std::fmt::Debug for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Reject for SendError {}

pub async fn run(
    commands: Commands,
    player_state: watch::Receiver<PlayerState>,
) -> anyhow::Result<()> {
    let handle_play_pause = warp::path!("play_pause").map(|| Command::PlayPause);
    let handle_previous_item = warp::path!("previous_item").map(|| Command::PreviousItem);
    let handle_next_item = warp::path!("next_item").map(|| Command::NextItem);
    let handle_set_volume = warp::path!("set_volume" / i32).map(Command::SetVolume);

    let handle_commands = handle_play_pause
        .or(handle_previous_item)
        .unify()
        .or(handle_next_item)
        .unify()
        .or(handle_set_volume)
        .unify();

    let command_response = handle_commands.map(move |command| match commands.send(command) {
        Ok(()) => "OK".into_response(),
        Err(err) => reply::with_status(
            format!("{:?}", err),
            warp::http::StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into_response(),
    });

    let state_changes = warp::path!("state_changes").map(move || {
        use futures::StreamExt;

        let mut current_state: Option<PlayerState> = None;

        warp::sse::reply(
            warp::sse::keep_alive().stream(
                player_state
                    .clone()
                    .map(move |new_state: PlayerState| {
                        use warp::sse::ServerSentEvent;
                        let mut messages = Vec::new();

                        check_for_arc_change!(messages, current_state, new_state, pipeline_state);
                        check_for_json_arc_change!(
                            messages,
                            current_state,
                            new_state,
                            current_track
                        );
                        check_for_change!(messages, current_state, new_state, volume);
                        check_for_change!(messages, current_state, new_state, buffering);

                        current_state = Some(new_state);

                        futures::stream::iter(messages)
                    })
                    .flatten()
                    .map(Ok::<_, std::convert::Infallible>),
            ),
        )
    });

    let static_content =
        warp::fs::dir("static").or(warp::path::end().and(warp::fs::file("static/index.html")));

    let routes = command_response.or(state_changes).or(static_content);

    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;

    Ok(())
}
