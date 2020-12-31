use std::path::Path;

use rradio_messages::Track;

mod directory_search;

#[cfg(unix)]
mod unix;

#[cfg(unix)]
use unix::mount;

type Result<T> =
    std::result::Result<T, rradio_messages::MountError<crate::atomic_string::AtomicString>>;

pub struct Handle {
    _mount: sys_mount::UnmountDrop<sys_mount::Mount>,
    mounted_directory: tempdir::TempDir,
}

#[cfg(all(feature = "usb", unix))]
pub fn usb(device: &str) -> Result<(Handle, Vec<Track>)> {
    let handle = mount(device, "vfat", None)?;
    let tracks = random_music_directory(handle.mounted_directory.as_ref())?;
    Ok((handle, tracks))
}

#[cfg(all(feature = "samba", unix))]
pub fn samba(device: &str, credentials: &super::Credentials) -> Result<(Handle, Vec<Track>)> {
    let handle = mount(device, "cifs", Some(credentials))?;
    let tracks = random_music_directory(handle.mounted_directory.as_ref())?;
    Ok((handle, tracks))
}

fn random_music_directory(directory_path: &Path) -> Result<Vec<Track>> {
    directory_search::random_music_directory(directory_path)
        .map_err(|err| rradio_messages::MountError::ErrorFindingTracks(err.to_string().into()))?
        .ok_or(rradio_messages::MountError::TracksNotFound)
}
