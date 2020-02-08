use anyhow::Result;
use futures::StreamExt;
use gstreamer::GstObjectExt;

pub async fn main(bus: gstreamer::Bus) -> Result<()> {
    let mut messages = gstreamer::BusStream::new(&bus);

    while let Some(message) = messages.next().await {
        use gstreamer::MessageView;

        match message.view() {
            MessageView::Buffering(buff) => {
                println!("{:?}", buff.get_percent());
            }
            MessageView::Tag(tag) => {
                println!("tags");
                for (k, v) in tag.get_tags().as_ref().iter() {
                    println!("{} => {:?}", k, v.get::<String>());
                }
            }
            MessageView::NeedContext(_) => (),
            MessageView::HaveContext(_) => (),
            MessageView::Latency(_) => (),
            MessageView::AsyncStart(_) => (),
            MessageView::AsyncDone(_) => (),
            MessageView::StateChanged(_) => (),
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
            msg => println!("{:?}", msg),
        }
    }

    Ok(())
}
