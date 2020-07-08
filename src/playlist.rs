use anyhow::{Context, Error, Result};

#[derive(Clone, Debug)]
pub struct Entry {
    pub title: Option<String>,
    pub url: String,
    pub is_notification: bool,
}

fn entry_url(entry: m3u::Entry) -> Result<String> {
    Ok(match entry {
        m3u::Entry::Path(path) => {
            String::from(path.to_str().context(format!("Bad Path: {:?}", path))?)
        }
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
                    is_notification: false,
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
                        is_notification: false,
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
                    is_notification: false,
                })
                .collect()
        })
        .map_err(Error::new)
}

pub fn load(path: impl AsRef<std::path::Path> + Clone) -> Result<Vec<Entry>> {
    match path
        .as_ref()
        .extension()
        .context("File has no extension")?
        .to_string_lossy()
        .as_ref()
    {
        "m3u" => parse_m3u(path),
        "pls" => parse_pls(path),
        extension => Err(Error::msg(format!("Unsupported format: \"{}\"", extension))),
    }
}
