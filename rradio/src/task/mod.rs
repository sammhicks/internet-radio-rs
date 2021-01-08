//! Utilities for managing concurrent tasks

mod log_error;
mod shutdown;
mod wait_group;

pub use log_error::FailableFuture;
pub use shutdown::Signal as ShutdownSignal;
pub use wait_group::WaitGroup;
