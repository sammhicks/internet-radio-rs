use std::{fmt::Display, future::Future, task::Poll};

pub async fn log_error(future: impl Future<Output = anyhow::Result<()>>) {
    if let Err(err) = future.await {
        log::error!("{:#}", err);
    }
}

#[pin_project::pin_project]
pub struct FutureWithContext<
    F: Future<Output = anyhow::Result<()>>,
    C: Clone + Display + Send + Sync + 'static,
> {
    #[pin]
    future: F,
    context: C,
}

impl<F: Future<Output = anyhow::Result<()>>, C: Clone + Display + Send + Sync + 'static> Future
    for FutureWithContext<F, C>
{
    type Output = anyhow::Result<()>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        use anyhow::Context;
        let this = self.project();
        match this.future.poll(cx) {
            Poll::Ready(ready) => Poll::Ready(ready.context(this.context.clone())),
            Poll::Pending => Poll::Pending,
        }
    }
}

pub trait CanAttachContext: Future<Output = anyhow::Result<()>> + Sized {
    fn context<C: Clone + Display + Send + Sync + 'static>(
        self,
        context: C,
    ) -> FutureWithContext<Self, C> {
        FutureWithContext {
            future: self,
            context,
        }
    }
}

impl<T: Future<Output = anyhow::Result<()>> + Sized> CanAttachContext for T {}
