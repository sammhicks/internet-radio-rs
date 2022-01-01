use std::{
    ffi::OsString,
    io::Result,
    path::{Path, PathBuf},
};

use rand::{seq::SliceRandom, Rng};

use rradio_messages::Track;

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

pub fn random_music_directory(directory_path: &Path) -> Result<Option<Vec<Track>>> {
    let mut rng = rand::thread_rng();
    random_artist_directory(directory_path, &mut rng)
}

fn random_artist_directory(
    directory_path: &Path,
    rng: &mut impl Rng,
) -> Result<Option<Vec<Track>>> {
    log::debug!("Searching {}", directory_path.display());
    for directory in random_subdirectories(directory_path, rng)? {
        let artist = directory.file_name();
        let artist = artist.to_string_lossy();
        if let Some(playlist) = random_album_directory(&directory.path(), artist.as_ref(), rng)? {
            return Ok(Some(playlist));
        }
    }

    Ok(None)
}

fn random_album_directory(
    directory_path: &Path,
    artist: &str,
    rng: &mut impl Rng,
) -> Result<Option<Vec<Track>>> {
    log::debug!("Searching {}", directory_path.display());
    for directory in random_subdirectories(directory_path, rng)? {
        let album = directory.file_name();
        let album = album.to_string_lossy();
        if let Some(playlist) = album_directory(&directory.path(), artist, album.as_ref())? {
            return Ok(Some(playlist));
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

fn random_track(
    directory_path: &Path,
    artist: &str,
    album: &str,
    rng: &mut impl Rng,
) -> Result<Option<Track>> {
    album_directory(directory_path, artist, album).map(|tracks| {
        tracks.and_then(|mut tracks| {
            if tracks.is_empty() {
                None
            } else {
                let index = rng.gen_range(0..tracks.len());
                Some(tracks.remove(index))
            }
        })
    })
}

fn random_directory(
    directory_path: &Path,
    rng: &mut impl Rng,
) -> Result<Option<(OsString, PathBuf)>> {
    let mut subdirectories = Vec::new();

    for item in std::fs::read_dir(directory_path)? {
        let item = item?;
        if item.file_type()?.is_dir() {
            subdirectories.push(item);
        }
    }

    Ok(if subdirectories.is_empty() {
        None
    } else {
        let index = rng.gen_range(0..subdirectories.len());
        let item = subdirectories.remove(index);
        Some((item.file_name(), item.path()))
    })
}

pub fn shuffled_mixed_playlist(
    directory_path: &Path,
    track_count: usize,
) -> Result<Option<Vec<Track>>> {
    let mut rng = rand::thread_rng();

    let mut tracks = Vec::new();

    for _ in 0..track_count {
        if let Some((artist, artist_directory_path)) = random_directory(directory_path, &mut rng)? {
            if let Some((album, album_directory_path)) =
                random_directory(&artist_directory_path, &mut rng)?
            {
                let artist = artist.to_string_lossy();
                let album = album.to_string_lossy();
                if let Some(track) = random_track(&album_directory_path, &artist, &album, &mut rng)?
                {
                    tracks.push(track);
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
