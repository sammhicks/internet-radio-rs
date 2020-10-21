use std::future::Future;
use std::sync::Arc;

use tokio::sync::oneshot;

#[derive(Clone)]
pub struct Handle(Arc<oneshot::Sender<()>>);

impl Handle {
    pub fn spawn_task<F: Future<Output = anyhow::Result<()>> + Send + 'static>(&self, f: F) {
        let handle = self.clone();
        tokio::spawn(async move {
            crate::log_error::log_error(f).await;
            drop(handle);
        });
    }
}

pub struct WaitGroup {
    handle: Handle,
    complete: oneshot::Receiver<()>,
}

impl WaitGroup {
    pub fn new() -> Self {
        let (handle, complete) = oneshot::channel();
        Self {
            handle: Handle(Arc::new(handle)),
            complete,
        }
    }

    pub fn clone_handle(&self) -> Handle {
        self.handle.clone()
    }

    pub fn spawn_task<F: Future<Output = anyhow::Result<()>> + Send + 'static>(&self, f: F) {
        self.handle.spawn_task(f);
    }

    pub async fn wait(self) {
        drop(self.handle);
        self.complete.await.ok();
    }
}
