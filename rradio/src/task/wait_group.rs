//! Wait for all tasks to complete

use std::future::Future;
use std::sync::Arc;

use tracing::Instrument;

use super::shutdown;

struct TaskIsActive(Handle);

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
        if let Some(TaskIsActive(handle)) = self.is_active.take() {
            tracing::warn!(id = handle.id(), "Task has been aborted");
        }
    }
}

struct HandleCore {
    id: usize,
    #[allow(dead_code)]
    handle: shutdown::Handle,
}

/// A handle which is dropped when the corresponding task is terminated
#[derive(Clone)]
pub struct Handle(Arc<HandleCore>);

impl Handle {
    fn id(&self) -> usize {
        self.0.id
    }

    /// Spawn a new task using the same wait group as this handle
    pub fn spawn_task(
        &self,
        span: tracing::Span,
        task: impl Future<Output = anyhow::Result<()>> + Send + 'static,
    ) {
        let handle = self.clone();
        tokio::spawn(
            async move {
                let log_dropped_task = LogDroppedTask {
                    is_active: Some(TaskIsActive(handle)),
                };
                let result = task.await;
                log_dropped_task.shutdown();

                result
            }
            .instrument(span),
        );
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        tracing::trace!(
            id = self.id(),
            strong_count = Arc::strong_count(&self.0),
            "About to drop handle"
        );
    }
}

/// A `WaitGroup` allows a task to wait for multiple other tasks to terminate
pub struct WaitGroup {
    handle: Handle,
    complete: shutdown::Signal,
}

impl WaitGroup {
    pub fn new() -> Self {
        static ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

        let (handle, complete) = shutdown::Signal::new();

        Self {
            handle: Handle(Arc::new(HandleCore {
                id: ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst),
                handle,
            })),
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
        let id = Arc::as_ptr(&self.handle.0) as usize;

        tracing::trace!(
            id,
            strong_count = Arc::strong_count(&self.handle.0),
            "Waiting for Wait Group"
        );

        drop(self.handle);

        self.complete.await;

        tracing::trace!(?id, "Success");
    }
}
