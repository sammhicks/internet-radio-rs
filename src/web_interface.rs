use tokio::sync::{mpsc, watch};
use warp::{
    reject::Reject,
    reply::{self, Reply},
    Filter,
};

use crate::{message::Command, pipeline::PlayerState};

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
    let handle_set_volume = warp::path!("set_volume" / i32).map(Command::SetVolume);

    let handle_commands = handle_play_pause.or(handle_set_volume).unify();

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
        warp::sse::reply(
            warp::sse::keep_alive().stream(player_state.clone().map(|new_state| {
                Ok::<_, std::convert::Infallible>(warp::sse::data(format!("{:?}", new_state)))
            })),
        )
    });

    let static_content =
        warp::fs::dir("static").or(warp::path::end().and(warp::fs::file("static/index.html")));

    let routes = command_response.or(state_changes).or(static_content);

    warp::serve(routes).run(([127, 0, 0, 1], 3030)).await;

    Ok(())
}
