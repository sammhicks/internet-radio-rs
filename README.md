# internet-radio-rs

## Build Options using Environment Variables

+ `RRADIO_CONFIG_PATH` - The default location of the config path if not set using command line option `-c`. Defaults to `config.toml`

## Example Config file
    stations_directory = "stations"
    input_timeout = "2s"
    volume_offset = 5
    buffering_duration = "40s"
    log_level = "Info"

    [Notifications]
    ready = "file:///usr/share/sounds/success.mp3"
    error = "file:///usr/share/sounds/error.mp3"


Options:
+ stations_directory
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
+ pause_before_playing_increment
  + Default: `"1s"`
  + The additional amount to wait if an infinite stream terminates unexpectedly before attempting to reconnect
+ smart_goto_previous_track_duration
  + Default: `"2s"`
  + The amount of time at the start of a track where winding back will take you to the previous track, rather than seeking to the start of the current track
+ max_pause_before_playing
  + Default: `"5s"`
  + The maximum amount to wait if an infinite stream terminates unexpectedly before giving up
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
  + Values:
    + `ready` - Ready for input
    + `playlist_prefix` - Played before the station tracks
    + `playlist_suffix` - Played after the station tracks
    + `error` - Played when an error occurs

## Optional Features

+ `cd` - Support playing CDs
  + Additional config:
    ```
    [CD]
    device = "/dev/cdrom"
    ```
+ `production-server` - Bind to `0.0.0.0` over TCP
+ `samba` - Support playing music from remote samba servers
+ `usb` - Support playing music from usb devices
+ `web` (Enabled by default) - Support for a web interface
  + `production-server` - Bind to port `80`
+ `ping` - Ping the gateway and remote servers to diagnose connection problems
  + Additional config:
    ```
    ping_count = 30
    gateway_address = "192.168.0.1"
    initial_ping_address = "8.8.8.8"
    ```
