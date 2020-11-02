# internet-radio-rs

## Build Options using Environment Variables

+ `RRADIO_CONFIG_PATH` - The default location of the config path if not set using command line option `-c`. Defaults to `config.toml`

## Example Config file
    station_directory = "stations"
    input_timeout = "2s"
    volume_offset = 5
    buffering_duration = "40s"
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
+ input_timeout
  + Default: `"2s"`
  + Station indexes are two digits. This is the timeout between the first digit and the second. Uses [`humantime`](https://docs.rs/humantime/2.0.1/humantime/)
+ volume_offset
  + Default: `5`
  + The default volume change when incrementing and decrementing the volume
+ buffering_duration
  + Default: `"2s"`
  + The gstreaming buffer duration
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
