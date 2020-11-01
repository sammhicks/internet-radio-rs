use std::fmt::{Debug, Display, Formatter};

use anyhow::Result;

use rradio_messages::Command;

use super::BroadcastEvent;

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

impl<'a, S: AsRef<str> + Debug, TrackList: AsRef<[rradio_messages::Track]>> Display
    for DisplayDiff<&'a rradio_messages::PlayerStateDiff<S, TrackList>>
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

        let current_track_index_row = station_row + station_row_count;
        let current_track_index_row_count = 1;
        if let Some(track_index) = self.0.current_track_index {
            Display::fmt(&MoveTo(0, current_track_index_row), f)?;
            display_entry(f, "Current Track", track_index)?;
        }

        let tags_row = current_track_index_row + current_track_index_row_count;
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
        let buffering_row_count = 1;
        if let Some(buffering) = self.0.buffering {
            Display::fmt(&MoveTo(0, buffering_row), f)?;
            display_entry(f, "Buffering", buffering)?;
        }

        let track_duration_row = buffering_row + buffering_row_count;
        let track_duration_row_count = 1;
        if let Some(duration) = self.0.track_duration.into_option() {
            Display::fmt(&MoveTo(0, track_duration_row), f)?;
            display_entry(f, "Duration", duration)?;
        }

        let track_position_row = track_duration_row + track_duration_row_count;
        // let track_position_row_count = 1;
        if let Some(position) = self.0.track_position.into_option() {
            Display::fmt(&MoveTo(0, track_position_row), f)?;
            display_entry(f, "Position", position)?;
        }

        Ok(())
    }
}

fn encode_message(message: &BroadcastEvent) -> Result<Vec<u8>> {
    Ok(match message {
        BroadcastEvent::ProtocolVersion(_) => format!(
            "{}{}{}{}Version {}",
            crossterm::cursor::Hide,
            crossterm::terminal::SetSize(120, 16),
            crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
            crossterm::cursor::MoveTo(0, 0),
            rradio_messages::VERSION
        )
        .into_bytes(),
        BroadcastEvent::PlayerStateChanged(diff) => DisplayDiff(diff).to_string().into_bytes(),
        BroadcastEvent::LogMessage(log_message) => {
            format!("{}{:?}", crossterm::cursor::MoveTo(0, 0), log_message).into_bytes()
        }
    })
}

fn decode_command(
    stream: tokio::net::tcp::OwnedReadHalf,
) -> impl futures::Stream<Item = Result<Command>> {
    futures::stream::unfold(stream, |mut stream| async move {
        use tokio::io::AsyncReadExt;

        let mut buffer = [0; 64];
        loop {
            match stream.read(&mut buffer).await {
                Ok(0) => return None,
                Err(err) => return Some((Err(anyhow::Error::new(err)), stream)),
                Ok(_) => continue,
            }
        }
    })
}

pub async fn run(port_channels: super::PortChannels) -> Result<()> {
    super::tcp::run(
        port_channels,
        std::module_path!(),
        8001,
        encode_message,
        decode_command,
    )
    .await
}
