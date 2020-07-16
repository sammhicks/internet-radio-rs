#![macro_use]

use tokio::sync::{mpsc, watch};
use warp::{
    reject::{Reject, Rejection},
    reply::{self, Reply},
    Filter,
};

use crate::message::{Command, PlayerState};

#[allow(unused_macros)]
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

#[derive(Debug)]
enum ParseError<E: std::fmt::Debug + Sized + Send + Sync + 'static> {
    NotUtf8(std::str::Utf8Error),
    FromStrErr(E),
}

impl<E: std::fmt::Debug + Sized + Send + Sync + 'static> Reject for ParseError<E> {}

fn body<T>() -> impl Filter<Extract = (T,), Error = Rejection> + Copy
where
    T: std::str::FromStr + Send,
    T::Err: std::fmt::Debug + Sized + Send + Sync + 'static,
{
    warp::filters::body::bytes().and_then(|body_bytes: bytes::Bytes| async move {
        let body_str = std::str::from_utf8(body_bytes.as_ref())
            .map_err(|err| warp::reject::custom(ParseError::NotUtf8::<T::Err>(err)))?;

        body_str
            .parse()
            .map_err(|err| warp::reject::custom(ParseError::FromStrErr(err)))
    })
}

struct SendError(mpsc::error::SendError<Command>);

impl std::fmt::Debug for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl Reject for SendError {}

#[cfg(feature = "embed_static_web_content")]
macro_rules! mime_type {
    (html) => {
        "text/html; charset=utf-8"
    };
    (css) => {
        "text/css"
    };
    (js) => {
        "application/javascript"
    };
}

#[cfg(feature = "embed_static_web_content")]
macro_rules! static_item_with_filter {
    ($path_filter:expr, $file_name:expr, $file_type:ident) => {
        $path_filter.map(|| {
            warp::reply::with_header(
                include_str!(concat!("../static/", $file_name)),
                warp::http::header::CONTENT_TYPE,
                mime_type!($file_type),
            )
        })
    };
}

#[cfg(not(feature = "embed_static_web_content"))]
macro_rules! static_item_with_filter {
    ($path_filter:expr, $file_name:expr, $file_type_unused:ident) => {
        $path_filter.and(warp::fs::file(concat!("static/", $file_name)))
    };
}

macro_rules! static_item {
    ($item:expr, $file_type:ident) => {
        static_item_with_filter!(warp::path($item).and(warp::path::end()), $item, $file_type)
    };
}

macro_rules! static_page {
    ($item:expr) => {
        static_item!(concat!($item, ".html"), html)
            .or(static_item!(concat!($item, ".css"), css))
            .or(static_item!(concat!($item, ".js"), js))
    };
}

macro_rules! static_pages {
    [$($item:expr),*] => {
        static_item_with_filter!(warp::path::end(), "index.html", html) $(.or(static_page!($item)))*
    };
}

async fn handle_rejection(err: Rejection) -> Result<impl Reply, std::convert::Infallible> {
    use warp::http::StatusCode;

    log::error!("{:?}", err);

    let code;

    if err.is_not_found() {
        code = StatusCode::NOT_FOUND;
    } else if err.find::<ParseError<std::num::ParseIntError>>().is_some(){
        code = StatusCode::BAD_REQUEST;
    } else if err.find::<warp::reject::MethodNotAllowed>().is_some() {
        code = StatusCode::METHOD_NOT_ALLOWED;
    } else {
        code = StatusCode::INTERNAL_SERVER_ERROR;
    }

    Ok(warp::reply::with_status(code.to_string(), code))
}

pub async fn run(
    commands: mpsc::UnboundedSender<Command>,
    player_state: watch::Receiver<PlayerState>,
) -> anyhow::Result<()> {
    let handle_play_pause = warp::path!("play_pause").map(|| Command::PlayPause);
    let handle_previous_item = warp::path!("previous_item").map(|| Command::PreviousItem);
    let handle_next_item = warp::path!("next_item").map(|| Command::NextItem);
    let handle_set_volume = warp::path!("volume").and(warp::post()).and(body()).map(Command::SetVolume);
    let handle_play_url = warp::path!("play_url").and(warp::post()).and(body()).map(Command::PlayUrl);

    let handle_commands = handle_play_pause
        .or(handle_previous_item)
        .unify()
        .or(handle_next_item)
        .unify()
        .or(handle_set_volume)
        .unify()
        .or(handle_play_url)
        .unify()
        .and(warp::post());

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

                        check_for_change!(messages, current_state, new_state, pipeline_state);
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

    let static_content = static_pages!["index", "podcasts"].or(static_item!("common.js", js));

    let routes = command_response
        .or(state_changes)
        .or(static_content)
        .recover(handle_rejection);

    #[cfg(feature = "production_web_server")]
    let addr = (std::net::Ipv4Addr::UNSPECIFIED, 80);

    #[cfg(not(feature = "production_web_server"))]
    let addr = (std::net::Ipv4Addr::LOCALHOST, 3030);

    warp::serve(routes).run(addr).await;

    Ok(())
}
