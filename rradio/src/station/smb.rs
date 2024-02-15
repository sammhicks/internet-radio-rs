use std::{
    iter::FromIterator,
    path::{Path, PathBuf},
};

use rradio_messages::Track;

use super::{mount::Mount, PlaylistHandle, PlaylistMetadata};

use std::fmt;

use anyhow::{Context, Result};
use rand::seq::SliceRandom;
use rradio_messages::StationIndex;

#[derive(Clone, serde::Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials").finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum SortBy {
    Name,
    Random,
}

#[derive(serde::Deserialize)]

struct PlaylistDescription {
    title: String,
    share: String,
    #[serde(flatten)]
    credentials: Credentials,
    playlist: Option<String>,
    #[serde(default)]
    sort_by: Option<SortBy>,
    #[serde(default)]
    limit_track_count: Option<usize>,
}

#[derive(Debug)]
pub struct Station {
    index: StationIndex,
    title: String,
    share: String,
    credentials: Credentials,
    playlist: Option<String>,
    sort_by: Option<SortBy>,
    limit_track_count: Option<usize>,
}

impl Station {
    fn from_file(path: &std::path::Path, index: StationIndex) -> Result<Self> {
        let PlaylistDescription {
            title,
            share,
            credentials,
            playlist,
            sort_by,
            limit_track_count,
        } = toml::from_str(
            &std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read {}", path.display()))?,
        )
        .with_context(|| format!("Failed to parse {}", path.display()))?;

        Ok(Self {
            index,
            title,
            share,
            credentials,
            playlist,
            sort_by,
            limit_track_count,
        })
    }

    pub fn index(&self) -> &StationIndex {
        &self.index
    }

    pub fn title(&self) -> &str {
        &self.title
    }
}

pub fn from_file(path: &std::path::Path, index: StationIndex) -> Result<super::Station> {
    Station::from_file(path, index).map(super::Station::Smb)
}

struct SmbMount<'a> {
    share_path: &'a str,
    data: &'a str,
}

impl<'a> Mount for SmbMount<'a> {
    fn device(&self) -> &str {
        self.share_path
    }

    fn file_system_type(&self) -> &str {
        "cifs"
    }

    fn maybe_data(&self) -> Option<&str> {
        Some(self.data)
    }
}

#[derive(Clone)]
struct Metadata {
    track_paths: Vec<String>,
}

impl super::TypeName for Metadata {
    const TYPE_NAME: &'static str = "SMB Metadata";
}

fn playlist_files(
    root_directory: &Path,
    playlist_path: String,
) -> Result<Vec<String>, rradio_messages::MountError> {
    let playlist_file =
        std::fs::read_to_string(root_directory.join(&playlist_path)).map_err(|err| {
            tracing::error!(
                ?root_directory,
                playlist_path,
                "Could not open playlist: {err}"
            );
            rradio_messages::MountError::TracksNotFound
        })?;

    let playlist_folder = std::path::Path::new(&playlist_path)
        .parent()
        .unwrap_or(std::path::Path::new(""));

    let playlist_files = playlist_file
        .lines()
        .map(str::trim)
        .filter_map(|line| {
            (!line.is_empty() && !line.starts_with('#')).then(|| {
                playlist_folder
                    .join(line.replace('\\', "/"))
                    .to_string_lossy()
                    .into_owned()
            })
        })
        .collect();

    tracing::warn!(?playlist_files, "Playlist");

    Ok(playlist_files)
}

fn all_files(root_directory: &Path) -> Vec<String> {
    let mut directories = vec![PathBuf::new()];
    let mut files = Vec::new();

    while let Some(directory) = directories.pop() {
        let absolute_path = PathBuf::from_iter([root_directory, &directory]);
        let Ok(entries) = std::fs::read_dir(&absolute_path).map_err(|err| {
            tracing::warn!(
                "Failed to read directory {}: {err}",
                absolute_path.display()
            );
        }) else {
            continue;
        };

        for entry in entries {
            let Ok(entry) = entry.map_err(|err| {
                tracing::warn!(
                    "Failed to iterate over directory {}: {err}",
                    absolute_path.display()
                );
            }) else {
                break;
            };

            let Ok(entry_type) = entry.file_type().map_err(|err| {
                tracing::warn!("Failed to get type of {}: {err}", entry.path().display());
            }) else {
                continue;
            };

            if entry_type.is_dir() {
                directories.push(PathBuf::from_iter([
                    directory.as_path(),
                    entry.file_name().as_ref(),
                ]));
            }

            if entry_type.is_file() {
                if mime_guess::from_path(entry.file_name())
                    .iter()
                    .any(|mime_type| mime_type.type_() == "audio")
                {
                    let file_path_from_root_directory =
                        PathBuf::from_iter([directory.as_path(), entry.file_name().as_ref()]);

                    let Some(file_path_from_root_directory) =
                        file_path_from_root_directory.to_str()
                    else {
                        tracing::warn!("Invalid path: {}", file_path_from_root_directory.display());
                        continue;
                    };

                    files.push(file_path_from_root_directory.to_owned());
                }
            }
        }
    }

    files
}

fn track(mounted_directory: &Path, path: &str) -> Track {
    Track {
        title: None,
        album: None,
        artist: None,
        url: rradio_messages::arcstr::format!("file://{}/{}", mounted_directory.display(), path),
        is_notification: false,
    }
}

impl Station {
    pub fn into_playlist(
        self,
        metadata: Option<&super::PlaylistMetadata>,
    ) -> Result<super::Playlist, rradio_messages::MountError> {
        let Self {
            index,
            title,
            share,
            credentials: Credentials { username, password },
            playlist,
            sort_by,
            limit_track_count,
        } = self;

        let handle = SmbMount {
            share_path: &share,
            data: &format!("vers=3.0,user={username},password={password}"),
        }
        .mount()?;

        let (tracks, metadata) =
            if let Some(metadata) = metadata.and_then(PlaylistMetadata::get::<Metadata>) {
                (
                    metadata
                        .track_paths
                        .iter()
                        .map(|path| track(handle.mounted_directory(), path))
                        .collect(),
                    PlaylistMetadata::new(metadata),
                )
            } else {
                let mut track_paths = if let Some(playlist) = playlist {
                    playlist_files(handle.mounted_directory(), playlist)?
                } else {
                    all_files(handle.mounted_directory())
                };

                match sort_by {
                    None => (),
                    Some(SortBy::Name) => track_paths.sort(),
                    Some(SortBy::Random) => track_paths.shuffle(&mut rand::thread_rng()),
                }

                if let Some(limit_track_count) = limit_track_count {
                    while track_paths.len() > limit_track_count {
                        track_paths.pop();
                    }
                }

                (
                    track_paths
                        .iter()
                        .map(|path| track(handle.mounted_directory(), path))
                        .collect(),
                    PlaylistMetadata::new(Metadata { track_paths }),
                )
            };

        Ok(super::Playlist {
            station_index: Some(index),
            station_title: Some(title),
            station_type: rradio_messages::StationType::SambaShare,
            tracks,
            metadata,
            handle: PlaylistHandle::new(handle),
        })
    }
}
