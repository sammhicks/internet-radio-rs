use anyhow::{Context, Result};

use rradio_messages::Command;

fn encode_event_length(
    len: usize,
) -> Result<[u8; std::mem::size_of::<rradio_messages::MsgPackBufferLength>()]> {
    use std::convert::TryFrom;

    Ok(rradio_messages::MsgPackBufferLength::try_from(len)
        .with_context(|| format!("Failed to encode event length of {}", len))?
        .to_be_bytes())
}

fn encode_event(message: &super::BroadcastEvent) -> Result<Vec<u8>> {
    let mut message_buffer = rmp_serde::to_vec(message).context("Failed to encode event")?;

    let mut buffer = Vec::from(encode_event_length(message_buffer.len())?);
    buffer.append(&mut message_buffer);

    Ok(buffer)
}

fn extract_eof<T: std::fmt::Debug>(result: Result<T>) -> Option<Result<T>> {
    match result {
        Ok(success) => Some(Ok(success)),
        Err(err) => {
            if let Some(io_error) = err.downcast_ref::<std::io::Error>() {
                if let std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::UnexpectedEof =
                    io_error.kind()
                {
                    return None;
                }
            }

            Some(Err(err))
        }
    }
}

async fn read_command<Stream: tokio::io::AsyncRead + Unpin>(
    stream: &mut Stream,
) -> Result<Command> {
    use tokio::io::AsyncReadExt;

    let mut byte_count_buffer = [0; std::mem::size_of::<rradio_messages::MsgPackBufferLength>()];

    stream
        .read_exact(&mut byte_count_buffer)
        .await
        .context("Failed to read command message size")?;

    let byte_count = rradio_messages::MsgPackBufferLength::from_be_bytes(byte_count_buffer);

    let mut buffer = vec![0; byte_count as usize];

    stream
        .read_exact(&mut buffer)
        .await
        .context("Failed to read message")?;

    rmp_serde::from_slice(buffer.as_ref()).context("Failed to decode msgpack")
}

fn decode_command(
    stream: tokio::net::tcp::OwnedReadHalf,
) -> impl futures::Stream<Item = Result<Command>> {
    futures::stream::unfold(stream, |mut stream| async move {
        extract_eof(read_command(&mut stream).await).map(|value| (value, stream))
    })
}

pub async fn run(port_channels: super::PortChannels) {
    super::tcp::run(
        port_channels,
        std::module_path!(),
        8002,
        encode_event,
        decode_command,
    )
    .await;
}
