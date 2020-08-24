# internet-radio-rs

## Example Config file
    channels_directory = "channels"
    input_timeout_ms = 1000
    log_level = "Info"

    [Notifications]
    success = "file:///usr/share/sounds/success.mp3"
    error = "file:///usr/share/sounds/error.mp3"


Options:
+ channels_directory
  + Default: `"channels"`
  + A directory where channel playlists are found. The filename must start with the two digits of the channel, and must have an appropriate file extension.
  + Supported formats:
    + `.m3u` - https://en.wikipedia.org/wiki/M3U
    + `.pls` - https://en.wikipedia.org/wiki/PLS_(file_format)
+ input_timeout_ms
  + Default: `2000`
  + Channels are two digits. This is the timeout is milliseconds between the first digit and the second.
+ log_level
  + Default: `"Warn"`
  + Options:
    + `"Off"`
    + `"Error"`
    + `"Warn"`
    + `"Info"`
    + `"Debug"`
    + `"Trace"`
  + You can also specify per-module logging, using the same format as passed to [env_logger](https://docs.rs/env_logger/*/env_logger/)
+ Notifications
  + Default: None
  + Notification sounds to play on success or error

+ sample compile option
cargo run --release --all-features 
+ to allow program to open port 80
sudo setcap cap_net_bind_service=+ep target/release/rradio
sudo setcap cap_net_bind_service=+ep target/debug/rradio
