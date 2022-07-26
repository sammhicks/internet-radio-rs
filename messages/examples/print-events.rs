use anyhow::Context;
use futures_util::StreamExt;

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

    // Get the event stream
    let events =
        rradio_messages::Event::decode_from_stream(tokio::io::BufReader::new(connection_rx))
            .await?;

    // Disconnect from rradio on ctrl-c
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();

        drop(connection_tx);
    });

    // Print all events
    tokio::pin!(events);
    while let Some(event) = events.next().await.transpose()? {
        println!("{:?}", event);
    }

    Ok(())
}
