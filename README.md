# internet-radio-rs

## Example Config file
    channels_directory = "channels"
    input_timeout_ms = 1000
    log_level = "Info"

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
  + Default: `"Error"`
  + Options:
    + `"Off"`
    + `"Error"`
    + `"Warn"`
    + `"Info"`
    + `"Debug"`
    + `"Trace"`
