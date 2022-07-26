use futures::{SinkExt, TryStreamExt};
use tracing::Instrument;

pub async fn run(port_channels: super::PortChannels) {
    super::tcp::run(
        port_channels,
        rradio_messages::API_PORT,
        |stream| rradio_messages::Event::encode_to_stream(stream).sink_err_into(),
        |stream| {
            rradio_messages::Command::decode_from_stream(tokio::io::BufReader::new(stream))
                .err_into()
        },
    )
    .instrument(tracing::info_span!("tcp_binary"))
    .await;
}
