use anyhow::{Context, Result};

use rradio_messages::Command;
use tokio::io::AsyncBufReadExt;
use tracing::Instrument;

fn encode_event<'a>(message: &rradio_messages::Event, buffer: &'a mut Vec<u8>) -> Result<&'a [u8]> {
    message.encode(buffer).context("Failed to encode Event")
}

fn decode_commands(
    stream: tokio::net::tcp::OwnedReadHalf,
) -> impl futures::Stream<Item = Result<Command>> {
    futures::stream::try_unfold(
        (tokio::io::BufReader::new(stream), Vec::new()),
        |(mut stream, mut buffer)| async move {
            buffer.clear();
            match stream.read_until(0, &mut buffer).await {
                Ok(0) => Ok(None),
                Err(err) if err.kind() == std::io::ErrorKind::ConnectionReset => Ok(None),
                Err(err) => Err(anyhow::Error::new(err)),
                Ok(_) => {
                    let command = rradio_messages::Command::decode(&mut buffer)
                        .context("Failed to decode postcard")?;
                    Ok(Some((command, (stream, buffer))))
                }
            }
        },
    )
}

pub async fn run(port_channels: super::PortChannels) {
    super::tcp::run(
        port_channels,
        rradio_messages::API_PORT,
        encode_event,
        decode_commands,
    )
    .instrument(tracing::info_span!("tcp_postcard"))
    .await;
}
