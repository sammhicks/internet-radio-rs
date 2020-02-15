use anyhow::Result;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use log::error;

pub struct RawMode;

impl RawMode {
    pub fn new() -> Result<Self> {
        enable_raw_mode()?;

        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        if let Err(err) = disable_raw_mode() {
            error!("Cannot disable raw mode: {}", err);
        }
    }
}
