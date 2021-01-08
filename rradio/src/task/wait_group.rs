//! Wait for all tasks to complete

use std::future::Future;
use std::sync::Arc;

use tokio::sync::oneshot;

/// A handle which is dropped when the corresponding task is terminated
#[derive(Clone)]
pub struct Handle(Arc<oneshot::Sender<()>>);

impl Handle {
    /// Spawn a new task using the same wait group as this handle
    pub fn spawn_task<F: Future<Output = ()> + Send + 'static>(&self, f: F) {
        let handle = self.clone();
        tokio::spawn(async move {
            f.await;
            drop(handle);
        });
    }
}

/// A `WaitGroup` allows a task to wait for multiple other tasks to terminate
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

    /// Create a copy of the handle
    pub fn clone_handle(&self) -> Handle {
        self.handle.clone()
    }

    /// Spawn a task which the group will wait for
    pub fn spawn_task<F: Future<Output = ()> + Send + 'static>(&self, f: F) {
        self.handle.spawn_task(f);
    }

    /// Wait for all spawned tasks to terminate
    pub async fn wait(self) {
        drop(self.handle);
        self.complete.await.ok();
    }
}
