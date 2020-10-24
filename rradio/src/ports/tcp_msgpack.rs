use anyhow::Result;

use rradio_messages::Command;

fn encode_event(message: &super::BroadcastEvent) -> Result<Vec<u8>> {
    use std::convert::TryFrom;

    let mut message_buffer = rmp_serde::to_vec(message)?;

    let mut buffer = Vec::from(u16::try_from(message_buffer.len())?.to_be_bytes());
    buffer.append(&mut message_buffer);

    Ok(buffer)
}

fn extract_eof<T: std::fmt::Debug>(result: Result<T>) -> Option<Result<T>> {
    match result {
        Ok(success) => Some(Ok(success)),
        Err(err) => {
            if let Some(io_error) = err.downcast_ref::<std::io::Error>() {
                if let std::io::ErrorKind::UnexpectedEof = io_error.kind() {
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

    let byte_count = stream.read_u16().await?;

    let mut buffer = vec![0; byte_count as usize];

    stream.read_exact(&mut buffer).await?;

    rmp_serde::from_slice(buffer.as_ref()).map_err(anyhow::Error::new)
}

fn decode_command(
    stream: tokio::net::tcp::OwnedReadHalf,
) -> impl futures::Stream<Item = Result<Command>> {
    futures::stream::unfold(stream, |mut stream| async move {
        extract_eof(read_command(&mut stream).await).map(|value| (value, stream))
    })
}

pub async fn run(port_channels: super::PortChannels) -> Result<()> {
    super::tcp::run(
        port_channels,
        std::module_path!(),
        8002,
        encode_event,
        decode_command,
    )
    .await
}
