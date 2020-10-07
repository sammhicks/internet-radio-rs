# internet-radio-rs

## Build Options using Environment Variables

+ `RRADIO_CONFIG_PATH` - The default location of the config path if not set using command line option `-c`. Defaults to `config.toml`

## Example Config file
    station_directory = "stations"
    input_timeout_ms = 1000
    log_level = "Info"

    [Notifications]
    success = "file:///usr/share/sounds/success.mp3"
    error = "file:///usr/share/sounds/error.mp3"


Options:
+ channels_directory
  + Default: `"stations"`
  + A directory where radio stations are found. The filename must start with the two digits of the channel, and must have an appropriate file extension.
  + Supported formats:
    + `.m3u` - https://en.wikipedia.org/wiki/M3U
    + `.pls` - https://en.wikipedia.org/wiki/PLS_(file_format)
+ input_timeout_ms
  + Default: `2000`
  + Station indexes are two digits. This is the timeout is milliseconds between the first digit and the second.
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

## Optional Features

+ `cd` - Support playing CDs
