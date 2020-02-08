use anyhow::{Error, Result};
use gstreamer::ElementExt;

mod keyboard_events;
mod message_handler;
mod playbin;
mod print_value;

fn main() -> Result<()> {
    gstreamer::init()?;

    let playbin = playbin::Playbin::new()?;
    let bus = playbin.get_bus().ok_or(Error::msg("pipeline has no bus"))?;

    let mut rt = tokio::runtime::Runtime::new()?;

    rt.spawn(message_handler::main(bus));

    rt.block_on(keyboard_events::main(&playbin))
}
