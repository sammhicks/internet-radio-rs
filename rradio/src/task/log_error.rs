//! Allows tasks to log errors

use std::{fmt::Display, future::Future, pin::Pin, task::Poll};

pub struct FutureWithContext<F, C> {
    future: F,
    context: C,
}

impl<F, C> FutureWithContext<F, C> {
    fn project(self: Pin<&mut Self>) -> (Pin<&mut F>, &C) {
        unsafe {
            let this = self.get_unchecked_mut();
            (Pin::new_unchecked(&mut this.future), &this.context)
        }
    }
}

impl<F: Future<Output = anyhow::Result<()>>, C: Clone + Display + Send + Sync + 'static> Future
    for FutureWithContext<F, C>
{
    type Output = anyhow::Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let (future, context) = self.project();
        match future.poll(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(Ok(())),
            Poll::Ready(Err(err)) => Poll::Ready(Err(err.context(context.clone()))),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub struct LoggingFuture<F> {
    future: F,
    target: &'static str,
}

impl<F> LoggingFuture<F> {
    fn project(self: Pin<&mut Self>) -> (Pin<&mut F>, &'static str) {
        unsafe {
            let this = self.get_unchecked_mut();
            (Pin::new_unchecked(&mut this.future), this.target)
        }
    }
}

impl<F: Future<Output = anyhow::Result<()>>> Future for LoggingFuture<F> {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let (future, target) = self.project();
        match future.poll(cx) {
            Poll::Ready(Ok(())) => Poll::Ready(()),
            Poll::Ready(Err(err)) => {
                log::error!(target: target, "{:#}", err);
                Poll::Ready(())
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// A Future which can return an error
pub trait FailableFuture: Future<Output = anyhow::Result<()>> + Sized {
    /// Add context to errors produced by a task
    fn context<C: Clone + Display + Send + Sync + 'static>(
        self,
        context: C,
    ) -> FutureWithContext<Self, C> {
        FutureWithContext {
            future: self,
            context,
        }
    }

    /// Log and swallow errors produced by a task
    fn log_error(self, target: &'static str) -> LoggingFuture<Self> {
        LoggingFuture {
            future: self,
            target,
        }
    }
}

impl<T: Future<Output = anyhow::Result<()>> + Sized> FailableFuture for T {}
