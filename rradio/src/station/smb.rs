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

#[derive(Debug, Clone, Copy, Default, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum SortBy {
    #[default]
    Name,
    Random,
}

#[derive(serde::Deserialize)]

struct PlaylistDescription {
    title: String,
    share: String,
    #[serde(flatten)]
    credentials: Credentials,
    sort_by: SortBy,
    #[serde(default)]
    limit_track_count: Option<usize>,
}

#[derive(Debug)]
pub struct Station {
    index: StationIndex,
    title: String,
    share: String,
    credentials: Credentials,
    sort_by: SortBy,
    limit_track_count: Option<usize>,
}

impl Station {
    fn from_file(path: &std::path::Path, index: StationIndex) -> Result<Self> {
        let PlaylistDescription {
            title,
            share,
            credentials,
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
                let entry_name = entry.file_name();

                let Some(extension) = Path::new(&entry_name).extension() else {
                    continue;
                };

                let handled_extensions = ["mp3", "wma", "aac", "ogg", "wav"];

                if handled_extensions
                    .iter()
                    .any(|&handled_extension| extension == handled_extension)
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
                let mut track_paths = all_files(handle.mounted_directory());

                match sort_by {
                    SortBy::Name => track_paths.sort(),
                    SortBy::Random => match limit_track_count {
                        Some(limit_track_count) => {
                            track_paths.partial_shuffle(&mut rand::thread_rng(), limit_track_count);
                        }
                        None => track_paths.shuffle(&mut rand::thread_rng()),
                    },
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
            station_type: rradio_messages::StationType::UPnP,
            tracks,
            metadata,
            handle: PlaylistHandle::new(handle),
        })
    }
}
