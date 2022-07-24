use anyhow::{Context, Result};

use rradio_messages::{Command, Event};
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt};

pub fn encode_events<S: AsyncWrite + Unpin>(
    stream: S,
) -> impl futures::Sink<Event, Error = anyhow::Error> {
    futures::sink::unfold(
        (stream, Vec::new()),
        |(mut stream, mut buffer), event: Event| async move {
            buffer.clear();

            stream
                .write_all(event.encode(&mut buffer)?)
                .await
                .context("Failed to write event")?;

            Ok((stream, buffer))
        },
    )
}

pub fn decode_commands<S: AsyncRead + Unpin>(
    stream: S,
) -> impl futures::Stream<Item = Result<Command>> {
    futures::stream::try_unfold(
        (tokio::io::BufReader::new(stream), Vec::new()),
        |(mut stream, mut buffer)| async move {
            buffer.clear();
            match stream.read_until(0, &mut buffer).await {
                Ok(0) => Ok(None),
                Err(err) if err.kind() == std::io::ErrorKind::ConnectionReset => Ok(None),
                Err(err) => Err(anyhow::Error::new(err)),
                Ok(_) => Ok(Some((
                    rradio_messages::Command::decode(&mut buffer)?,
                    (stream, buffer),
                ))),
            }
        },
    )
}
