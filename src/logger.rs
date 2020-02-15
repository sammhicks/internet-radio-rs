#![allow(clippy::print_with_newline)]
use std::{io::Write, sync::Mutex};

use crossterm::{
    style::{Color, ResetColor, SetForegroundColor},
    ExecutableCommand,
};
use log::{Level, Metadata, Record};

pub struct Logger<W>(Mutex<W>);

impl<W> Logger<W> {
    pub fn new(writer: W) -> Box<Self> {
        Box::new(Self(Mutex::new(writer)))
    }
}

impl<W: Write + Send> log::Log for Logger<W> {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Trace
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            let mut writer = self.0.lock().unwrap();
            writer
                .execute(SetForegroundColor(match record.level() {
                    Level::Trace => Color::Magenta,
                    Level::Debug => Color::Blue,
                    Level::Info => Color::Green,
                    Level::Warn => Color::Yellow,
                    Level::Error => Color::Red,
                }))
                .unwrap();
            print!("{} - {}\r\n", record.level(), record.args());
            writer.execute(ResetColor).unwrap();
        }
    }

    fn flush(&self) {}
}
