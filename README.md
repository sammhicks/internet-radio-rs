# internet-radio-rs

## Build Options using Environment Variables

+ `RRADIO_CONFIG_PATH` - The default location of the config path if not set using command line option `-c`. Defaults to `config.toml`

### Notable Dependencies

Most dependencies are pure Rust, and thus Cargo handles them without problem.
However, there are a couple of exceptions

+ [gstreamer](https://gitlab.freedesktop.org/gstreamer/gstreamer-rs)
+ [pnet ("`ping`" feature only)](https://github.com/libpnet/libpnet)

## Example Config file
    stations_directory = "stations"
    input_timeout = "2s"
    volume_offset = 5
    buffering_duration = "40s"
    log_level = "info"

    [Notifications]
    ready = "file:///usr/share/sounds/success.mp3"
    error = "file:///usr/share/sounds/error.mp3"

    [CD]
    device = "/dev/cdrom"

    [ping]
    remote_ping_count = 10
    gateway_address = "192.168.0.1"
    initial_ping_address = "8.8.8.8"

    [web]
    web_app_path = "/var/www"


Options:
+ stations_directory
  + Default: `"stations"`
  + A directory where radio stations are found. The filename must start with the two digits of the channel, and must have an appropriate file extension.
  + Supported formats:
    + `.m3u` - https://en.wikipedia.org/wiki/M3U
    + `.pls` - https://en.wikipedia.org/wiki/PLS_(file_format)
    + `.upnp` - Custom Format; See Below
+ input_timeout
  + Default: `"2s"`
  + Station indexes are two digits. This is the timeout between the first digit and the second. Uses [`humantime`](https://docs.rs/humantime/2.0.1/humantime/)
+ initial_volume
  + Default: `70`
  + The volume when rradio starts up
+ volume_offset
  + Default: `5`
  + The default volume change when incrementing and decrementing the volume
+ buffering_duration
  + Default: `"2s"`
  + The gstreaming buffer duration
+ pause_before_playing_increment
  + Default: `"1s"`
  + The additional amount to wait if an infinite stream terminates unexpectedly before attempting to reconnect
+ max_pause_before_playing
  + Default: `"5s"`
  + The maximum amount to wait if an infinite stream terminates unexpectedly before giving up
+ smart_goto_previous_track_duration
  + Default: `"2s"`
  + The amount of time at the start of a track where winding back will take you to the previous track, rather than seeking to the start of the current track
+ log_level
  + Default: `"warn"`
  + Options:
    + `"off"`
    + `"error"`
    + `"warn"`
    + `"info"`
    + `"debug"`
    + `"trace"`
  + You can also specify per-module logging, see [tracing](https://docs.rs/tracing-subscriber/latest/tracing_subscriber/struct.EnvFilter.html)
+ Notifications
  + Default: None
  + Values:
    + `ready` - Ready for input
    + `playlist_prefix` - Played before the station tracks
    + `playlist_suffix` - Played after the station tracks
    + `error` - Played when an error occurs
+ play_error_sound_on_gstreamer_error
  + Default: true
  + If gstreamer raises an error, play the "Error" sound and then reset the playlist and stop playing
+ CD
  + Only if `cd` feature is enabled
  + Values:
    + station - The station which plays the cd
    + device - The cd drive device
  + Defaults:
    + station: `"00"`
    + device: `"/dev/cdrom"`
+ USB
  + Only if `usb` feature is enabled
  + Values:
    + station - The station which plays the usb
    + device - The usb device
    + path - The directory which contains Music. Tracks must be arranged first in a folder per artist, then inside a folder per album of that artist
  + Defaults:
    + station: `"01"`
    + device: `"/dev/sda1"`
    + path: `""`
+ ping
  + Only if `ping` feature is enabled
  + Values:
    + remote_ping_count - How many times to ping the remote server
    + gateway_address - The gateway address to ping
    + initial_ping_address - The device to ping on startup
  + Defaults:
    + remote_ping_count:
    + gateway_address: On unix, this is calculated from `/proc/net/route`. On windows: 127.0.0.1
    + initial_ping_address: `8.8.8.8`
+ web
  + Only if `web` feature is enabled
  + Values:
    + web_app_path - The path to find the static web app files
  + Defaults:
    + web_app_path: `web_app`

## UPnP Station Format

### Single Container

    [container]
    station_title = "UPnP - Single Container" # Optional, defaults to UPnP Media Server name
    root_description_url = "http://192.168.0.1:8200/rootDesc.xml"
    container = "Playlists/My Playlist"
    sort_by = "none" # Optional, defaults to "none"
    limit_track_count = 20 # Optional, defaults to unlimited
    filter_upnp_class = "object.item.audioItem.musicTrack" # Optional, defaults to not filtering


+ `root_description_url` - The url pointing to the root description url of the UPnP device
+ `container` - The "path" of the container, i.e. a forward slash delimitered list of container names
+ `sort_by` - How to sort the tracks
  + `none` - As per the Content Directory. This is the default
  + `track_number`
  + `random`

### Random Container

    [random_container]
    station_title = "UPnP - Random Container"
    root_description_url = "http://192.168.0.1:8200/rootDesc.xml"
    container = "Album"
    sort_by = "none" # Optional, defaults to "none"
    limit_track_count = 20 # Optional, defaults to unlimited

Same as per `[container]`, but a random container inside the specified container is chosen and all of its items are played

### Flattened Container

    [flattened_container]
    station_title = "UPnP - Flattened Container"
    root_description_url = "http://192.168.0.1:8200/rootDesc.xml"
    container = "Folders/AC_DC"
    sort_by = "random"
    limit_track_count = 20 # Optional, defaults to unlimited

Same as per `[container]`, but the playlist contains all tracks contained within subcontainers of the selected container

## Optional Features

+ `cd` - Support playing CDs
+ `production-server` - Bind to `0.0.0.0` over TCP
+ `usb` - Support playing music from usb devices
+ `web` (Enabled by default) - Support for a web interface
  + `production-server` - Bind to port `80`
+ `ping` - Ping the gateway and remote servers to diagnose connection problems
