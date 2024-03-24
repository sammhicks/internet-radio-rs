use std::{
    iter::FromIterator,
    path::{Path, PathBuf},
};

use super::mount::{Handle, Mount};

use std::fmt;

use rand::seq::SliceRandom;

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
pub struct Metadata {
    track_paths: Vec<String>,
}

impl super::TypeName for Metadata {
    const TYPE_NAME: &'static str = "SMB Metadata";
}

fn playlist_files(
    root_directory: &Path,
    playlist_path: &str,
) -> Result<Vec<String>, rradio_messages::MountError> {
    let playlist_file =
        std::fs::read_to_string(root_directory.join(playlist_path)).map_err(|err| {
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
        .filter(|&line| (!line.is_empty() && !line.starts_with('#')))
        .map(|line| {
            playlist_folder
                .join(line.replace('\\', "/"))
                .to_string_lossy()
                .into_owned()
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

            if entry_type.is_file()
                && mime_guess::from_path(entry.file_name())
                    .iter()
                    .any(|mime_type| mime_type.type_() == "audio")
            {
                let file_path_from_root_directory =
                    PathBuf::from_iter([directory.as_path(), entry.file_name().as_ref()]);

                let Some(file_path_from_root_directory) = file_path_from_root_directory.to_str()
                else {
                    tracing::warn!("Invalid path: {}", file_path_from_root_directory.display());
                    continue;
                };

                files.push(file_path_from_root_directory.to_owned());
            }
        }
    }

    files
}

pub struct Track {
    url: rradio_messages::ArcStr,
}

impl Track {
    fn new(mounted_directory: &Path, path: &str) -> Self {
        Self {
            url: rradio_messages::arcstr::format!(
                "file://{}/{}",
                mounted_directory.display(),
                path
            ),
        }
    }
}

impl From<Track> for super::Track {
    fn from(Track { url }: Track) -> Self {
        Self {
            title: None,
            album: None,
            artist: None,
            url,
            is_notification: false,
        }
    }
}

pub struct Loader {
    pub path: std::path::PathBuf,
}

impl super::StationLoader for Loader {
    type Metadata = Metadata;
    type Handle = Handle;
    type Track = Track;
    type Error = super::Error;

    const STATION_TYPE: rradio_messages::StationType = rradio_messages::StationType::Usb;

    async fn load_station_parts(
        self,
        metadata: Option<Self::Metadata>,
        publish_station_info: impl FnOnce(super::PartialInfo),
    ) -> Result<(Option<super::StationTitle>, Vec<Track>, Metadata, Handle), super::Error> {
        use anyhow::Context;

        let Self { path } = self;

        let PlaylistDescription {
            title,
            share,
            credentials: Credentials { username, password },
            playlist,
            sort_by,
            limit_track_count,
        } = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))
            .and_then(|playlist_description| {
                toml::from_str(&playlist_description)
                    .with_context(|| format!("Failed to parse {}", path.display()))
            })
            .map_err(|error| super::BadStationFile { error })?;

        publish_station_info(super::PartialInfo {
            title: Some(&title),
        });

        let handle = SmbMount {
            share_path: &share,
            data: &format!("vers=3.0,user={username},password={password}"),
        }
        .mount()?;

        let new_metadata = if let Some(metadata) = metadata {
            metadata
        } else {
            let mut track_paths = if let Some(playlist_path) = playlist {
                playlist_files(handle.mounted_directory(), &playlist_path)?
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

            Metadata { track_paths }
        };

        let tracks = new_metadata
            .track_paths
            .iter()
            .map(|path| Track::new(handle.mounted_directory(), path))
            .collect();

        Ok((
            Some(super::StationTitle {
                station_title: title,
            }),
            tracks,
            new_metadata,
            handle,
        ))
    }
}
