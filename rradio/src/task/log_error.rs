//! Allows tasks to log errors

use std::future::Future;

pub struct LoggingFuture<F>(F);

impl<F: Future<Output = anyhow::Result<()>>> Future for LoggingFuture<F> {
    type Output = ();

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        unsafe { self.map_unchecked_mut(|LoggingFuture(err)| err) }
            .poll(cx)
            .map_err(|err| tracing::error!("{:#}", err))
            .map(|_: Result<(), ()>| ())
    }
}

/// A Future which can return an error
pub trait FailableFuture: Future<Output = anyhow::Result<()>> + Sized {
    /// Log and swallow errors produced by a task
    fn log_error(self) -> tracing::instrument::Instrumented<LoggingFuture<Self>> {
        use tracing::Instrument;
        LoggingFuture(self).instrument(tracing::Span::current())
    }
}

impl<T: Future<Output = anyhow::Result<()>> + Sized> FailableFuture for T {}
