use std::io::Result;
use std::path::Path;

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

fn random_subdirectories<R: Rng>(
    directory_path: &Path,
    rng: &mut R,
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

fn random_artist_directory<R: Rng>(
    directory_path: &Path,
    rng: &mut R,
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

fn random_album_directory<R: rand::Rng>(
    directory_path: &Path,
    artist: &str,
    rng: &mut R,
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
                    let title = name.to_string_lossy().into_owned();
                    log::debug!("Track: {}", title);

                    let mut url = String::from("file://");
                    url.push_str(file_path.to_string_lossy().as_ref());

                    tracks.push(Track {
                        title: Some(title),
                        album: Some(album.to_owned()),
                        artist: Some(artist.to_owned()),
                        url,
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
