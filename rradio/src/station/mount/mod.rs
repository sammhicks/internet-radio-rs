use std::{any::Any, path::Path};

use rradio_messages::Track;

mod directory_search;

use directory_search::SelectedDirectories;

#[cfg(all(unix, feature = "usb"))]
mod unix;

#[cfg(all(unix, feature = "usb"))]
use unix::mount;

type Result<T> = std::result::Result<T, rradio_messages::MountError>;

struct Handle {
    _mount: sys_mount::UnmountDrop<sys_mount::Mount>,
    mounted_directory: tempdir::TempDir,
}

#[cfg(all(feature = "usb", unix))]
pub fn usb(
    device: &str,
    path: &Path,
    metadata: Option<&super::PlaylistMetadata>,
) -> Result<(Vec<Track>, super::PlaylistMetadata, super::PlaylistHandle)> {
    let handle = mount(device, "vfat", None)?;

    let mut directory = std::path::PathBuf::from(handle.mounted_directory.path());
    directory.push(path);

    let (tracks, selected_directories) = random_music_directory(
        &directory,
        metadata.and_then(|super::PlaylistMetadata(metadata)| {
            metadata
                .as_ref()
                .downcast_ref::<SelectedDirectories>()
                .or_else(|| {
                    log::error!(
                        "metadata is not SelectedDirectories, but is {:?}",
                        metadata.type_id()
                    );

                    None
                })
        }),
    )?;
    Ok((
        tracks,
        super::PlaylistMetadata::new(selected_directories),
        super::PlaylistHandle::new(handle),
    ))
}

fn random_music_directory(
    directory_path: &Path,
    selected_directories: Option<&SelectedDirectories>,
) -> Result<(Vec<Track>, SelectedDirectories)> {
    directory_search::random_music_directory(directory_path, selected_directories)
        .map_err(|err| rradio_messages::MountError::ErrorFindingTracks(err.to_string().into()))?
        .ok_or(rradio_messages::MountError::TracksNotFound)
}
