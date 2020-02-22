use std::path::Path;

use anyhow::Result;

use crate::playlist::Entry;

#[derive(Clone, Debug)]
pub struct Channel {
    pub index: String,
    pub playlist: Vec<Entry>,
}

impl Channel {
    pub fn load(directory: impl AsRef<Path>, index: String) -> Result<Self> {
        for entry in std::fs::read_dir(directory)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();

            if name.starts_with(&index) {
                return Ok(Self {
                    index,
                    playlist: crate::playlist::load(entry.path())?,
                });
            }
        }

        Err(anyhow::Error::msg(format!("Channel {} not found", index)))
    }

    pub fn start_with_notification(mut self, url: Option<String>) -> Self {
        if let Some(url) = url {
            self.playlist.insert(
                0,
                crate::playlist::Entry {
                    title: None,
                    url: url.clone(),
                },
            )
        };
        self
    }
}
