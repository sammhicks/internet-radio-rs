use std::sync::Arc;

use tokio::sync::Mutex;

pub struct Signal {
    mutex: Arc<Mutex<()>>,
    _lock: tokio::sync::OwnedMutexGuard<()>,
}

impl Signal {
    pub fn new() -> Self {
        let mutex = Arc::new(Mutex::new(()));
        let lock = mutex.clone().try_lock_owned().unwrap();
        Self { mutex, _lock: lock }
    }

    pub fn wait(&self) -> impl std::future::Future<Output = ()> {
        let mutex = self.mutex.clone();
        async move {
            mutex.lock().await;
        }
    }
}

impl Default for Signal {
    fn default() -> Self {
        Self::new()
    }
}
