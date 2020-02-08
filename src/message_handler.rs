use anyhow::Result;
use futures::StreamExt;
use glib::value::ToValue;
use gstreamer::{GstObjectExt, MessageView, State};

use super::print_value::value_to_string;

pub async fn main(bus: gstreamer::Bus) -> Result<()> {
    let mut messages = gstreamer::BusStream::new(&bus);

    let mut previous_state_change = None;

    while let Some(message) = messages.next().await {
        match message.view() {
            MessageView::Buffering(buff) => {
                println!("{:?}", buff.get_percent());
            }
            MessageView::Tag(tag) => {
                println!("tags:");
                for (k, v) in tag.get_tags().as_ref().iter() {
                    println!("\t{} => {:?}", k, value_to_string(&v.to_value()));
                }
            }
            MessageView::StateChanged(state_change) => {
                let new_state = state_change.get_current();

                if previous_state_change != Some(new_state) {
                    println!("State Changed: {:?}", new_state);

                    previous_state_change = Some(new_state);
                }
            }
            MessageView::NewClock(_) => (),
            MessageView::DurationChanged(_) => (),
            MessageView::NeedContext(_) => (),
            MessageView::HaveContext(_) => (),
            MessageView::Latency(_) => (),
            MessageView::AsyncStart(_) => (),
            MessageView::AsyncDone(_) => (),
            // MessageView::StateChanged(_) => (),
            MessageView::StreamStart(_) => (),
            MessageView::StreamStatus(_) => (),
            MessageView::Element(_) => (),
            MessageView::Qos(_) => (),
            MessageView::Eos(_) => println!("Done"),
            MessageView::Error(err) => {
                println!(
                    "Error from {:?}: {} (({:?}))",
                    err.get_src().map(|s| s.get_path_string()),
                    err.get_error(),
                    err.get_debug()
                );
            }
            msg => println!("Unknown Message: {:?}", msg),
        }
    }

    Ok(())
}
