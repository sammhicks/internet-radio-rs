use anyhow::{Context, Error, Result};

#[derive(Clone, Debug)]
pub struct Entry {
    pub title: Option<String>,
    pub url: String,
}

fn entry_url(entry: m3u::Entry) -> Result<String> {
    Ok(match entry {
        m3u::Entry::Path(path) => String::from(
            path.to_str()
                .ok_or_else(|| anyhow::Error::msg("Bad Path"))?,
        ),
        m3u::Entry::Url(url) => url.into_string(),
    })
}

fn parse_m3u(path: impl AsRef<std::path::Path> + Clone) -> Result<Vec<Entry>> {
    use m3u::EntryExtReaderConstructionError;
    match m3u::Reader::open_ext(path.clone()) {
        Ok(mut reader) => reader
            .entry_exts()
            .map(|entry| {
                let entry = entry?;
                Ok(Entry {
                    title: Some(entry.extinf.name),
                    url: entry_url(entry.entry)?,
                })
            })
            .collect(),
        Err(EntryExtReaderConstructionError::HeaderNotFound) => {
            let mut reader = m3u::Reader::open(path)?;

            reader
                .entries()
                .map(|entry| {
                    Ok(Entry {
                        title: None,
                        url: entry_url(entry?)?,
                    })
                })
                .collect()
        }
        Err(EntryExtReaderConstructionError::BufRead(err)) => {
            Err(err).context("Failed to read file")
        }
    }
}

fn parse_pls(path: impl AsRef<std::path::Path>) -> Result<Vec<Entry>> {
    let mut reader = std::fs::File::open(path)?;
    pls::parse(&mut reader)
        .map(|entries| {
            entries
                .into_iter()
                .map(|entry| Entry {
                    title: entry.title,
                    url: entry.path,
                })
                .collect()
        })
        .map_err(anyhow::Error::new)
}

pub fn parse(path: impl AsRef<std::path::Path> + Clone) -> Result<Vec<Entry>> {
    use std::ops::Deref;
    match path
        .as_ref()
        .extension()
        .ok_or_else(|| Error::msg("No extension"))?
        .to_string_lossy()
        .deref()
    {
        "m3u" => parse_m3u(path),
        "pls" => parse_pls(path),
        extension => Err(Error::msg(format!("Unsupported format: \"{}\"", extension))),
    }
}
