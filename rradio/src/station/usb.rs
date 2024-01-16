use std::{
    ffi::OsString,
    path::{Path, PathBuf},
};

use rand::{seq::SliceRandom, Rng};
use rradio_messages::Track;

use super::{mount::Mount, PlaylistHandle, PlaylistMetadata};

struct UsbMount<'a> {
    device: &'a str,
}

impl<'a> Mount for UsbMount<'a> {
    fn device(&self) -> &str {
        self.device
    }

    fn file_system_type(&self) -> &str {
        "vfat"
    }
}

#[derive(Clone)]
pub struct SelectedDirectories {
    artist: OsString,
    album: OsString,
}

impl super::TypeName for SelectedDirectories {
    const TYPE_NAME: &'static str = "SelectedDirectories";
}

pub fn load(
    device: &str,
    path: &Path,
    metadata: Option<&PlaylistMetadata>,
) -> Result<(Vec<Track>, PlaylistMetadata, PlaylistHandle), rradio_messages::MountError> {
    let handle = UsbMount { device }.mount()?;

    let mut directory = std::path::PathBuf::from(handle.mounted_directory());
    directory.push(path);

    let (tracks, selected_directories) = random_music_directory(
        &directory,
        metadata.and_then(PlaylistMetadata::get::<SelectedDirectories>),
    )
    .map_err(|err| {
        rradio_messages::MountError::ErrorFindingTracks(rradio_messages::arcstr::format!("{err}"))
    })?
    .ok_or(rradio_messages::MountError::TracksNotFound)?;
    Ok((
        tracks,
        PlaylistMetadata::new(selected_directories),
        PlaylistHandle::new(handle),
    ))
}

fn filter_directory(
    item: std::io::Result<std::fs::DirEntry>,
) -> std::io::Result<Option<std::fs::DirEntry>> {
    let item = item?;
    if item.file_type()?.is_dir() {
        Ok(Some(item))
    } else {
        Ok(None)
    }
}

fn random_subdirectories(
    directory_path: &Path,
    rng: &mut impl Rng,
) -> std::io::Result<Vec<std::fs::DirEntry>> {
    let mut subdirectories = std::fs::read_dir(directory_path)?
        .filter_map(|item| filter_directory(item).transpose())
        .collect::<std::io::Result<Vec<_>>>()?;

    subdirectories.as_mut_slice().shuffle(rng);

    Ok(subdirectories)
}

pub fn random_music_directory(
    directory_path: &Path,
    selected_directories: Option<SelectedDirectories>,
) -> std::io::Result<Option<(Vec<Track>, SelectedDirectories)>> {
    match selected_directories {
        Some(selected_directories) => {
            let SelectedDirectories { album, artist } = &selected_directories;

            let mut directory_path = PathBuf::from(directory_path);
            directory_path.push(artist);
            directory_path.push(album);

            let artist = artist.to_string_lossy();
            let album = album.to_string_lossy();

            album_directory(&directory_path, &artist, &album)
                .map(|tracks| tracks.map(|tracks| (tracks, selected_directories)))
        }
        None => random_artist_directory(directory_path, &mut rand::thread_rng()),
    }
}

fn random_artist_directory(
    directory_path: &Path,
    rng: &mut impl Rng,
) -> std::io::Result<Option<(Vec<Track>, SelectedDirectories)>> {
    tracing::debug!("Searching {}", directory_path.display());
    for directory in random_subdirectories(directory_path, rng)? {
        let artist_directory_name = directory.file_name();
        let artist = artist_directory_name.to_string_lossy().into_owned();
        if let Some(playlist) =
            random_album_directory(&directory.path(), artist_directory_name, &artist, rng)?
        {
            return Ok(Some(playlist));
        }
    }

    Ok(None)
}

fn random_album_directory(
    directory_path: &Path,
    artist_directory_name: OsString,
    artist: &str,
    rng: &mut impl Rng,
) -> std::io::Result<Option<(Vec<Track>, SelectedDirectories)>> {
    tracing::debug!("Searching {}", directory_path.display());
    for directory in random_subdirectories(directory_path, rng)? {
        let album_directory_name = directory.file_name();
        let album = album_directory_name.to_string_lossy();
        if let Some(playlist) = album_directory(&directory.path(), artist, &album)? {
            return Ok(Some((
                playlist,
                SelectedDirectories {
                    album: album_directory_name,
                    artist: artist_directory_name,
                },
            )));
        }
    }

    Ok(None)
}

fn album_directory(
    directory_path: &Path,
    artist: &str,
    album: &str,
) -> std::io::Result<Option<Vec<Track>>> {
    tracing::debug!("Creating playlist from {}", directory_path.display());
    let handled_extensions = ["mp3", "wma", "aac", "ogg", "wav"];

    let mut tracks = Vec::new();

    for item in std::fs::read_dir(directory_path)? {
        let item = item?;
        if item.file_type()?.is_file() {
            let file_path = item.path();
            if let Some((name, extension)) = file_path.file_stem().zip(file_path.extension()) {
                if handled_extensions
                    .iter()
                    .any(|handled_extension| handled_extension == &extension)
                {
                    let title = name.to_string_lossy();
                    tracing::debug!("Track: {}", title);

                    tracks.push(Track {
                        title: Some(title.into()),
                        album: Some(album.into()),
                        artist: Some(artist.into()),
                        url: rradio_messages::arcstr::format!(
                            "file://{}",
                            file_path.to_string_lossy()
                        ),
                        is_notification: false,
                    });
                }
            }
        }
    }

    Ok(if tracks.is_empty() {
        None
    } else {
        Some(tracks)
    })
}
