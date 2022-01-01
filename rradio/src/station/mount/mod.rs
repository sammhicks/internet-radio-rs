use std::path::Path;

use rradio_messages::Track;

mod directory_search;

#[cfg(all(unix, any(feature = "usb", feature = "samba")))]
mod unix;

#[cfg(all(unix, any(feature = "usb", feature = "samba")))]
use unix::mount;

type Result<T> = std::result::Result<T, rradio_messages::MountError>;

pub struct Handle {
    _mount: sys_mount::UnmountDrop<sys_mount::Mount>,
    mounted_directory: tempdir::TempDir,
}

#[cfg(all(feature = "usb", unix))]
pub fn usb(device: &str, shuffle: Option<usize>) -> Result<(Handle, Vec<Track>)> {
    let handle = mount(device, "vfat", None)?;
    let tracks = random_music_directory(handle.mounted_directory.as_ref(), shuffle)?;
    Ok((handle, tracks))
}

#[cfg(all(feature = "samba", unix))]
pub fn samba(
    device: &str,
    credentials: &super::Credentials,
    shuffle: Option<usize>,
) -> Result<(Handle, Vec<Track>)> {
    let handle = mount(device, "cifs", Some(credentials))?;
    let tracks = random_music_directory(handle.mounted_directory.as_ref(), shuffle)?;
    Ok((handle, tracks))
}

fn random_music_directory(directory_path: &Path, shuffle: Option<usize>) -> Result<Vec<Track>> {
    match shuffle {
        None => directory_search::random_music_directory(directory_path),
        Some(track_count) => directory_search::shuffled_mixed_playlist(directory_path, track_count),
    }
    .map_err(|err| rradio_messages::MountError::ErrorFindingTracks(err.to_string().into()))?
    .ok_or(rradio_messages::MountError::TracksNotFound)
}
