pub struct ConnectionStream(pub tokio::net::TcpListener);

impl tokio::stream::Stream for ConnectionStream {
    type Item = std::io::Result<(tokio::net::TcpStream, std::net::SocketAddr)>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.0.poll_accept(cx).map(Some)
    }
}
