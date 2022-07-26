use anyhow::Context;
use futures_util::{SinkExt, StreamExt};

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    // Get the host from command line arguments, default to localhost
    let host = std::env::args().nth(1);
    let host = host.as_deref().unwrap_or("localhost");

    let rradio_address = (host, rradio_messages::API_PORT);

    println!("Connecting to {:?}", rradio_address);

    // Connect to rradio
    let (connection_rx, connection_tx) = tokio::net::TcpStream::connect(rradio_address)
        .await
        .with_context(|| format!("Failed to connect to {}", host))?
        .into_split();

    // Get the event stream. This must be done before sending a command to ensure that the correct version of rradio is used
    let events =
        rradio_messages::Event::decode_from_stream(tokio::io::BufReader::new(connection_rx))
            .await?;

    // Send PlayPause
    {
        let commands = rradio_messages::Command::encode_to_stream(connection_tx);
        tokio::pin!(commands);
        commands.send(rradio_messages::Command::PlayPause).await?;
    }

    // Ignore all events. Even if a client doesn't react to events, it must empty the read side of the connection
    tokio::pin!(events);
    while events.next().await.transpose()?.is_some() {}

    Ok(())
}
