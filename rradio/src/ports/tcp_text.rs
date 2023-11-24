use std::fmt::{Debug, Display, Formatter};

use anyhow::{Context, Result};

use rradio_messages::{Command, Event};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tracing::Instrument;

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

impl<'a> Display for DisplayDiff<&'a rradio_messages::PlayerStateDiff> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        use crossterm::cursor::MoveTo;

        let state_row = 0;
        let state_row_count = 1;
        if let Some(state) = self.0.pipeline_state {
            Display::fmt(&MoveTo(0, state_row), f)?;
            display_entry(f, "Pipeline State", state)?;
        }

        let station_row = state_row + state_row_count;
        let station_row_count = 2;
        if let Some(current_station) = &self.0.current_station {
            match current_station {
                rradio_messages::CurrentStation::NoStation => {
                    clear_lines(f, station_row, station_row_count)?;
                }
                rradio_messages::CurrentStation::FailedToPlayStation { error } => {
                    clear_lines(f, station_row, station_row_count)?;
                    display_entry(f, "Failed to play station", error)?;
                }
                rradio_messages::CurrentStation::LoadingStation { index, title, .. }
                | rradio_messages::CurrentStation::PlayingStation { index, title, .. } => {
                    Display::fmt(&MoveTo(0, station_row), f)?;
                    display_entry(f, "Station Index", index)?;
                    display_entry(f, "Station Title", title)?;
                }
            }
        }

        let pause_before_playing_row = station_row + station_row_count;
        let pause_before_playing_row_count = 1;
        if let Some(pause_before_playing) = self.0.pause_before_playing {
            Display::fmt(&MoveTo(0, pause_before_playing_row), f)?;
            display_entry(f, "Pause Before Playing", pause_before_playing)?;
        }

        let current_track_index_row = pause_before_playing_row + pause_before_playing_row_count;
        let current_track_index_row_count = 1;
        if let Some(track_index) = self.0.current_track_index {
            Display::fmt(&MoveTo(0, current_track_index_row), f)?;
            display_entry(f, "Current Track", track_index)?;
        }

        let tags_row = current_track_index_row + current_track_index_row_count;
        let tags_row_count = 6;
        if let Some(current_track_tags) = &self.0.current_track_tags {
            match current_track_tags {
                None => clear_lines(f, tags_row, tags_row_count)?,
                Some(tags) => {
                    Display::fmt(&MoveTo(0, tags_row), f)?;
                    display_entry(f, "Title", &tags.title)?;
                    display_entry(f, "Organisation", &tags.organisation)?;
                    display_entry(f, "Artist", &tags.artist)?;
                    display_entry(f, "Album", &tags.album)?;
                    display_entry(f, "Genre", &tags.genre)?;
                    display_entry(f, "Comment", &tags.comment)?;
                }
            }
        }

        let volume_row = tags_row + tags_row_count;
        let volume_row_count = 1;
        if let Some(volume) = self.0.volume {
            Display::fmt(&MoveTo(0, volume_row), f)?;
            display_entry(f, "Volume", volume)?;
        }

        let is_muted_row = volume_row + volume_row_count;
        let is_muted_row_count = 1;
        if let Some(is_muted) = self.0.is_muted {
            Display::fmt(&MoveTo(0, is_muted_row), f)?;
            display_entry(f, "Muted", is_muted)?;
        }

        let buffering_row = is_muted_row + is_muted_row_count;
        let buffering_row_count = 1;
        if let Some(buffering) = self.0.buffering {
            Display::fmt(&MoveTo(0, buffering_row), f)?;
            display_entry(f, "Buffering", buffering)?;
        }

        let track_duration_row = buffering_row + buffering_row_count;
        let track_duration_row_count = 1;
        if let Some(duration) = self.0.track_duration {
            Display::fmt(&MoveTo(0, track_duration_row), f)?;
            display_entry(f, "Duration", duration)?;
        }

        let track_position_row = track_duration_row + track_duration_row_count;
        let track_position_row_count = 1;
        if let Some(position) = self.0.track_position {
            Display::fmt(&MoveTo(0, track_position_row), f)?;
            display_entry(f, "Position", position)?;
        }

        let ping_time_row = track_position_row + track_position_row_count;
        // let ping_time_row_count = 1;
        if let Some(ping_times) = &self.0.ping_times {
            Display::fmt(&MoveTo(0, ping_time_row), f)?;
            display_entry(f, "Ping Time", ping_times)?;
        }

        Ok(())
    }
}

pub fn encode_events<S: AsyncWrite + Unpin>(
    stream: S,
) -> impl futures::Sink<Event, Error = anyhow::Error> {
    use std::io::Write;

    futures::sink::unfold(
        (stream, Vec::new()),
        |(mut stream, mut buffer), event: Event| async move {
            buffer.clear();

            match event {
                Event::PlayerStateChanged(diff) => write!(buffer, "{}", DisplayDiff(&diff)),
            }
            .context("Failed to encode event")?;

            stream
                .write_all(&buffer)
                .await
                .context("Failed to write event")?;

            Ok((stream, buffer))
        },
    )
}

fn decode_commands(
    stream: tokio::net::tcp::OwnedReadHalf,
) -> impl futures::Stream<Item = Result<Command>> {
    futures::stream::try_unfold(stream, |mut stream| async move {
        use tokio::io::AsyncReadExt;

        let mut buffer = [0; 64];
        while stream.read(&mut buffer).await? > 0 {}

        Ok(None)
    })
}

pub async fn run(port_channels: super::PortChannels) {
    super::tcp::run(port_channels, 8001, encode_events, decode_commands)
        .instrument(tracing::error_span!("tcp_text"))
        .await;
}
