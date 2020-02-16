use std::future::Future;

use tokio::runtime::Runtime;

pub trait TaskSpawner {
    fn spawn_named(
        &mut self,
        name: &'static str,
        f: impl Future<Output = impl TaskResult> + Send + 'static,
    );
}

impl TaskSpawner for Runtime {
    fn spawn_named(
        &mut self,
        name: &'static str,
        f: impl Future<Output = impl TaskResult> + Send + 'static,
    ) {
        self.spawn(async move { f.await.handle(name) });
    }
}

pub trait TaskResult {
    fn handle(self, name: &'static str);
}

impl TaskResult for () {
    fn handle(self, name: &'static str) {
        log::debug!("{} finished", name)
    }
}

impl TaskResult for anyhow::Result<()> {
    fn handle(self, name: &'static str) {
        use anyhow::Context;
        match self.context(name) {
            Ok(()) => log::debug!("{} finished", name),
            Err(err) => log::error!("{:?}", err),
        }
    }
}
