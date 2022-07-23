use tracing::Instrument;

pub async fn run(port_channels: super::PortChannels) {
    super::tcp::run(
        port_channels,
        rradio_messages::API_PORT,
        super::binary_stream::encode_event,
        super::binary_stream::decode_commands,
    )
    .instrument(tracing::info_span!("tcp_binary"))
    .await;
}
