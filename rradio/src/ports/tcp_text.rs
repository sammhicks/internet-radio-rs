use std::net::Shutdown;
use std::{
    fmt::{Debug, Display, Formatter},
    net::SocketAddr,
};

use anyhow::{Context, Result};
use futures::StreamExt;
use tokio::{
    io::AsyncWriteExt,
    net::TcpStream,
    sync::{broadcast, oneshot, watch},
};

use rradio_messages::LogMessage;

use crate::{
    atomic_string::AtomicString,
    log_error::CanAttachContext,
    pipeline::{LogMessageSource, PlayerState},
};

use super::{Event, ShutdownSignal, WaitGroup, WaitGroupHandle};

fn clear_lines(f: &mut Formatter, row: u16, count: u16) -> std::fmt::Result {
    use crossterm::terminal;
    Display::fmt(&crossterm::cursor::MoveTo(0, row), f)?;
    for _ in 0..count {
        Display::fmt(&terminal::Clear(terminal::ClearType::CurrentLine), f)?;
        f.write_str("\r\n")?;
    }

    Ok(())
}

fn display_entry(f: &mut Formatter, label: &str, entry: impl Debug) -> std::fmt::Result {
    use crossterm::terminal::{Clear, ClearType};
    write!(
        f,
        "{}: {:?}{}\r\n",
        label,
        entry,
        Clear(ClearType::UntilNewLine)
    )
}

struct DisplayDiff<T>(T);

impl<S: AsRef<str> + Debug, TrackList: AsRef<[rradio_messages::Track]>> Display
    for DisplayDiff<rradio_messages::PlayerStateDiff<S, TrackList>>
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        use crossterm::cursor::MoveTo;

        let state_row = 1;
        let state_row_count = 1;
        if let Some(state) = self.0.pipeline_state {
            Display::fmt(&MoveTo(0, state_row), f)?;
            display_entry(f, "Pipeline State", state)?;
        }

        let station_row = state_row + state_row_count;
        let station_row_count = 2;
        match &self.0.current_station {
            rradio_messages::OptionDiff::NoChange => (),
            rradio_messages::OptionDiff::ChangedToNone => {
                clear_lines(f, station_row, station_row_count)?
            }
            rradio_messages::OptionDiff::ChangedToSome(station) => {
                Display::fmt(&MoveTo(0, station_row), f)?;
                display_entry(f, "Station Index", &station.index)?;
                display_entry(f, "Station Title", &station.title)?;
            }
        }

        let tags_row = station_row + station_row_count;
        let tags_row_count = 6;
        match &self.0.current_track_tags {
            rradio_messages::OptionDiff::NoChange => (),
            rradio_messages::OptionDiff::ChangedToNone => clear_lines(f, tags_row, tags_row_count)?,
            rradio_messages::OptionDiff::ChangedToSome(tags) => {
                Display::fmt(&MoveTo(0, tags_row), f)?;
                display_entry(f, "Title", &tags.title)?;
                display_entry(f, "Organisation", &tags.organisation)?;
                display_entry(f, "Artist", &tags.artist)?;
                display_entry(f, "Album", &tags.album)?;
                display_entry(f, "Genre", &tags.genre)?;
                display_entry(f, "Comment", &tags.comment)?;
            }
        }

        let volume_row = tags_row + tags_row_count;
        let volume_row_count = 1;
        if let Some(volume) = self.0.volume {
            Display::fmt(&MoveTo(0, volume_row), f)?;
            display_entry(f, "Volume", volume)?;
        }

        let buffering_row = volume_row + volume_row_count;
        // let buffering_row_count = 1;
        if let Some(buffering) = self.0.buffering {
            Display::fmt(&MoveTo(0, buffering_row), f)?;
            display_entry(f, "Buffering", buffering)?;
        }

        Ok(())
    }
}

async fn write_diff<
    Stream: tokio::io::AsyncWrite + Unpin,
    S: AsRef<str> + Debug,
    TrackList: AsRef<[rradio_messages::Track]>,
>(
    stream: &mut Stream,
    diff: rradio_messages::PlayerStateDiff<S, TrackList>,
) -> std::io::Result<()> {
    stream
        .write_all(DisplayDiff(diff).to_string().as_bytes())
        .await
}

async fn client_connected(
    stream: TcpStream,
    remote_addr: SocketAddr,
    player_state: watch::Receiver<PlayerState>,
    log_message: broadcast::Receiver<LogMessage<AtomicString>>,
    shutdown_signal: ShutdownSignal,
    wait_handle: WaitGroupHandle,
) -> Result<()> {
    log::info!("Connection from {}", remote_addr);

    let (mut stream_rx, mut stream_tx) = stream.into_split();

    stream_tx
        .write_all(
            format!(
                "{}{}",
                crossterm::cursor::Hide,
                crossterm::terminal::SetSize(120, 15)
            )
            .as_bytes(),
        )
        .await?;

    let mut current_state = (*player_state.borrow()).clone();

    write_diff(&mut stream_tx, super::player_state_to_diff(&current_state)).await?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    wait_handle.spawn_task(
        async move {
            use tokio::io::AsyncReadExt;

            let mut buffer = [0_u8; 32];
            while stream_rx.read(&mut buffer).await? != 0 {}

            log::debug!("Disconnection from {}", remote_addr);

            stream_rx.as_ref().shutdown(Shutdown::Read)?;

            drop(shutdown_tx);
            Ok(())
        }
        .context(remote_addr),
    );

    let player_state = player_state.map(Event::StateUpdate);
    let log_message = log_message.into_stream().filter_map(|msg| async {
        match msg {
            Ok(msg) => Some(Event::LogMessage(msg)),
            Err(_) => None,
        }
    });

    let events = futures::stream::select(player_state, log_message)
        .take_until(shutdown_rx)
        .take_until(shutdown_signal.wait());

    pin_utils::pin_mut!(events);

    while let Some(event) = events.next().await {
        match event {
            Event::StateUpdate(new_state) => {
                write_diff(
                    &mut stream_tx,
                    super::diff_player_state(&current_state, &new_state),
                )
                .await?;

                current_state = new_state;
            }
            Event::LogMessage(message) => {
                stream_tx
                    .write_all(
                        format!("{}{:?}", crossterm::cursor::MoveTo(0, 0), message).as_bytes(),
                    )
                    .await?
            }
        };
    }

    log::debug!("Closing connection to {}", remote_addr);

    stream_tx.as_ref().shutdown(Shutdown::Write)?;

    drop(wait_handle);

    Ok(())
}

pub async fn run(
    player_state: watch::Receiver<PlayerState>,
    log_message_source: LogMessageSource,
    shutdown_signal: ShutdownSignal,
) -> Result<()> {
    let addr = std::net::Ipv4Addr::LOCALHOST;
    let port = 8001;
    let socket_addr = (addr, port);

    let wait_group = WaitGroup::new();

    let listener = super::connection_stream::ConnectionStream(
        tokio::net::TcpListener::bind(socket_addr)
            .await
            .with_context(|| format!("Cannot listen to {:?}", socket_addr))?,
    )
    .take_until(shutdown_signal.clone().wait());

    log::info!("Listening on {:?}", socket_addr);

    pin_utils::pin_mut!(listener);

    while let Some((connection, remote_addr)) = listener.next().await.transpose()? {
        wait_group.spawn_task(
            client_connected(
                connection,
                remote_addr,
                player_state.clone(),
                log_message_source.subscribe(),
                shutdown_signal.clone(),
                wait_group.clone_handle(),
            )
            .context(remote_addr),
        );
    }

    log::debug!("Shutting down");

    wait_group.wait().await;

    log::debug!("Shut down");

    Ok(())
}
