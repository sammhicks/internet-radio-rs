//! A task which processes incoming commands and gstreamer messages, and sends commands to the gstreamer pipeline

mod controller;
mod playbin;

pub use controller::run;
