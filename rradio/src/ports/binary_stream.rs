use anyhow::Result;

use rradio_messages::Command;
use tokio::io::{AsyncBufReadExt, AsyncRead};

pub fn encode_event<'a>(
    message: &rradio_messages::Event,
    buffer: &'a mut Vec<u8>,
) -> Result<&'a [u8]> {
    message.encode(buffer).map_err(anyhow::Error::new)
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
