//! Wait for all tasks to complete

use std::future::Future;
use std::sync::Arc;

use tokio::sync::oneshot;

use super::FailableFuture;

struct TaskIsActive;

struct LogDroppedTask {
    is_active: Option<TaskIsActive>,
}

impl LogDroppedTask {
    fn shutdown(mut self) {
        self.is_active.take();
    }
}

impl Drop for LogDroppedTask {
    fn drop(&mut self) {
        if let Some(TaskIsActive) = self.is_active.take() {
            tracing::warn!("Task has been aborted");
        }
    }
}

/// A handle which is dropped when the corresponding task is terminated
#[derive(Clone)]
pub struct Handle(Arc<oneshot::Sender<()>>);

impl Handle {
    /// Spawn a new task using the same wait group as this handle
    pub fn spawn_task(
        &self,
        span: tracing::Span,
        task: impl Future<Output = anyhow::Result<()>> + Send + 'static,
    ) {
        let handle = self.clone();
        tokio::task::spawn(async move {
            let log_dropped_task = LogDroppedTask {
                is_active: Some(TaskIsActive),
            };
            task.log_error(span).await;
            drop(handle);
            log_dropped_task.shutdown();
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
    pub fn spawn_task(
        &self,
        span: tracing::Span,
        task: impl Future<Output = anyhow::Result<()>> + Send + 'static,
    ) {
        self.handle.spawn_task(span, task);
    }

    /// Wait for all spawned tasks to terminate
    pub async fn wait(self) {
        drop(self.handle);
        self.complete.await.ok();
    }
}
