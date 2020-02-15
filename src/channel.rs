use std::path::Path;

use anyhow::Result;

use crate::playlist::Entry;

#[derive(Clone, Debug)]
pub struct Channel {
    pub index: String,
    pub playlist: Vec<Entry>,
}

pub fn load(directory: impl AsRef<Path>, index: String) -> Result<Channel> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if name.starts_with(&index) {
            return Ok(Channel {
                index,
                playlist: crate::playlist::parse(entry.path())?,
            });
        }
    }

    Err(anyhow::Error::msg(format!("Channel {} not found", index)))
}
