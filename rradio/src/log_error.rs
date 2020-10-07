pub async fn log_error(future: impl std::future::Future<Output = anyhow::Result<()>>) {
    if let Err(err) = future.await {
        log::error!("{:#}", err);
    }
}
