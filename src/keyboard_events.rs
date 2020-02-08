use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent};
use futures::StreamExt;

use super::command::{Command, CommandSender};

pub async fn main(channel: CommandSender) -> Result<()> {
    let mut events = EventStream::new();
    while let Some(event) = events.next().await {
        match event? {
            Event::Key(KeyEvent {
                code: KeyCode::Esc,
                modifiers: _,
            }) => break,
            Event::Key(KeyEvent {
                code: KeyCode::Char(' '),
                modifiers: _,
            }) => {
                if let Err(_) = channel.send(Command::PlayPause) {
                    break;
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char(c),
                modifiers: _,
            }) if c.is_ascii_digit() => {
                if let Err(_) = channel.send(Command::SetChannel(c.to_digit(10).unwrap() as u8)) {
                    break;
                }
            }
            e => eprintln!("Unhandled key: {:?}", e),
        }
    }

    Ok(())
}
