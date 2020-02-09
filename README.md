# internet-radio-rs

## Example Config file
    input_timeout_ms = 1000
    log_level = "Info"

    [[Station]]
    name = "Sintel Trailer"
    channel = 1
    url = "https://www.freedesktop.org/software/gstreamer-sdk/data/media/sintel_trailer-480p.webm"

Options:
+ input_timeout_ms
  + Default: `2000`
  + Channels are two digits. This is the timeout is milliseconds between the first digit and the second.
+ log_level
  + Default: `"Error"`
  + Options:
    + `"Off"`
    + `"Error"`
    + `"Warn"`
    + `"Info"`
    + `"Debug"`
    + `"Trace"`
