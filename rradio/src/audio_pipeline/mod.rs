//! A task which processes incoming commands and gstreamer messages, and sends commands to the gstreamer pipeline

mod controller;
mod playbin;

#[cfg(feature = "ping")]
mod ping;

pub use controller::{run, PlayerState};
