//! Notification of shutdown

use std::sync::Arc;

use tokio::sync::Semaphore;

/// A handle which notifies signals when it is dropped
pub struct Handle(Arc<Semaphore>);

impl std::ops::Drop for Handle {
    fn drop(&mut self) {
        // Max permits is usize::MAX >> 3
        self.0.add_permits(usize::MAX >> 4);
    }
}

/// A signal that the handle has been dropped
#[derive(Clone)]
pub struct Signal(Arc<Semaphore>);

impl Signal {
    pub fn new() -> (Handle, Self) {
        let semaphore = Arc::new(Semaphore::new(0));
        let handle = Handle(semaphore.clone());
        let signal = Signal(semaphore);
        (handle, signal)
    }

    /// Wait for the handle to be dropped
    pub async fn wait(self) {
        drop(self.0.acquire().await);
    }
}
