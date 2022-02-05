use std::{
    ffi::OsString,
    io::Result,
    path::{Path, PathBuf},
};

use rand::{seq::SliceRandom, Rng};

use rradio_messages::Track;

#[derive(Clone)]
pub struct SelectedDirectories {
    artist: OsString,
    album: OsString,
}

fn filter_directory(item: Result<std::fs::DirEntry>) -> Result<Option<std::fs::DirEntry>> {
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
) -> Result<Vec<std::fs::DirEntry>> {
    let mut subdirectories = std::fs::read_dir(directory_path)?
        .filter_map(|item| filter_directory(item).transpose())
        .collect::<Result<Vec<_>>>()?;

    subdirectories.as_mut_slice().shuffle(rng);

    Ok(subdirectories)
}

pub fn random_music_directory(
    directory_path: &Path,
    selected_directories: Option<&SelectedDirectories>,
) -> Result<Option<(Vec<Track>, SelectedDirectories)>> {
    match selected_directories {
        Some(selected_directories) => {
            let SelectedDirectories { album, artist } = selected_directories;

            let mut directory_path = PathBuf::from(directory_path);
            directory_path.push(artist);
            directory_path.push(album);

            let artist = artist.to_string_lossy();
            let album = album.to_string_lossy();

            album_directory(&directory_path, &artist, &album)
                .map(|tracks| tracks.map(|tracks| (tracks, selected_directories.clone())))
        }
        None => random_artist_directory(directory_path, &mut rand::thread_rng()),
    }
}

fn random_artist_directory(
    directory_path: &Path,
    rng: &mut impl Rng,
) -> Result<Option<(Vec<Track>, SelectedDirectories)>> {
    log::debug!("Searching {}", directory_path.display());
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
) -> Result<Option<(Vec<Track>, SelectedDirectories)>> {
    log::debug!("Searching {}", directory_path.display());
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

fn album_directory(directory_path: &Path, artist: &str, album: &str) -> Result<Option<Vec<Track>>> {
    log::debug!("Creating playlist from {}", directory_path.display());
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
                    log::debug!("Track: {}", title);

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
