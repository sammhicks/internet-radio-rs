use std::str::FromStr;

use anyhow::{Context, Result};

use super::{Credentials, Station, Track};

trait CaseInsensitiveStripPrefix: Sized {
    fn case_insensitive_strip_prefix(self, prefix: &str) -> Option<Self>;
}

impl<'a> CaseInsensitiveStripPrefix for &'a str {
    fn case_insensitive_strip_prefix(self, prefix: &str) -> Option<Self> {
        let mut self_chars = self.chars();

        for prefix_char in prefix.chars() {
            if self_chars.next()?.to_ascii_lowercase() != prefix_char {
                return None;
            }
        }

        Some(self_chars.as_str())
    }
}

// extract "value" from "# name = value" if command matches "name"
fn extract_command<'line>(line: &'line str, command: &str) -> Option<&'line str> {
    Some(
        line.strip_prefix('#')? // If line.strip_prefix('#') = None, return None
            .trim_start()
            .case_insensitive_strip_prefix(command)?
            .trim_start()
            .strip_prefix('=')?
            .trim(),
    )
}

fn extract_flag(line: &str, flag: &str) -> Option<()> {
    let remainder = line
        .strip_prefix('#')?
        .trim_start()
        .case_insensitive_strip_prefix(flag)?
        .trim_start();

    remainder.is_empty().then(|| ())
}

#[derive(Debug, thiserror::Error)]
enum CredentialsError {
    #[error("No username given")]
    NoUsername,
    #[error("No password given")]
    NoPassword,
}

fn create_credentials(
    username: Option<String>,
    password: Option<String>,
) -> Result<Option<Credentials>, CredentialsError> {
    match (username, password) {
        (Some(username), Some(password)) => Ok(Some(Credentials { username, password })),
        (Some(_), None) => Err(CredentialsError::NoPassword),
        (None, Some(_)) => Err(CredentialsError::NoUsername),
        (None, None) => Ok(None),
    }
}

#[derive(Debug, thiserror::Error)]
enum ParsePlaylistError {
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error("Playlist has error on line {line_number}: {line:?}")]
    BadPlaylistLine { line_number: usize, line: String },
    #[error("Playlist is empty")]
    EmptyPlaylist,
    #[error("Bad credentials: {0}")]
    BadCredentials(CredentialsError),
    #[error("Playlist has bad contents")]
    BadPlaylist,
}

pub fn parse(path: impl AsRef<std::path::Path> + Clone, index: String) -> Result<Station> {
    let playlist_file = std::fs::File::open(path).context("Could not open file")?;
    let buffered_file = std::io::BufReader::new(playlist_file);

    parse_data(buffered_file, index).map_err(anyhow::Error::new)
}

#[allow(clippy::too_many_lines)]
/// given the contents of a playlist file, returns the parsed version thereof in a Playlist enum. The contents of the enum depend on what info was found in the file.
fn parse_data(
    buffered_file: impl std::io::BufRead,
    index: String,
) -> Result<Station, ParsePlaylistError> {
    let mut title = None;
    let mut username = None;
    let mut password = None;
    let mut pause_before_playing = None;
    let mut cd_device = None;
    let mut usb_device = None;
    let mut url_list = Vec::new();
    let mut show_buffer = None;
    let mut http_found = false;
    let mut file_path_found = false;
    let mut shuffle = false;

    for (line_index, one_line) in buffered_file.lines().enumerate() {
        let one_line = one_line.map_err(ParsePlaylistError::IoError)?;
        let one_line = one_line.trim();

        if one_line.is_empty() {
            continue; //skip empty lines
        }

        if let Some(title_value) = extract_command(one_line, "title") {
            title = Some(title_value.to_string());
        } else if let Some(username_value) = extract_command(one_line, "username") {
            username = Some(username_value.to_string());
        } else if let Some(password_value) = extract_command(one_line, "password") {
            password = Some(password_value.to_string());
        } else if let Some(pause_before_playing_value) =
            extract_command(one_line, "pause_before_playing")
        {
            match pause_before_playing_value.parse() {
                Ok(value) => pause_before_playing = Some(std::time::Duration::from_secs(value)),
                Err(err) => log::error!("pause_before_playing not an integer: {:?}", err),
            };
        } else if let Some(show_buffer_value) = extract_command(one_line, "show_buffer") {
            match bool::from_str(show_buffer_value) {
                Ok(value) => show_buffer = Some(value),
                Err(err) => log::error!("show_buffer not a boolean: {:?}", err),
            };
        } else if let Some(()) = extract_flag(one_line, "shuffle") {
            shuffle = true;
        } else if let Some(comment) = one_line.strip_prefix('#') {
            log::error!("Found comment or unrecognised parameter '{:?}'", comment);
        } else if one_line.starts_with("http") {
            // caters for http:// and also https://
            url_list.push(one_line.to_string());
            http_found = true;
        } else if one_line.starts_with("//") {
            url_list.push(one_line.to_string());
            file_path_found = true;
        } else if let Some(device) = one_line.strip_prefix("cd:") {
            cd_device = Some(device.to_string());
        } else if one_line.starts_with("/dev/") {
            usb_device = Some(one_line.to_string());
        } else {
            return Err(ParsePlaylistError::BadPlaylistLine {
                line_number: line_index + 1,
                line: one_line.to_string(),
            });
        }
    }

    if url_list.is_empty() && cd_device.is_none() && usb_device.is_none() {
        return Err(ParsePlaylistError::EmptyPlaylist);
    }

    let credentials =
        create_credentials(username, password).map_err(ParsePlaylistError::BadCredentials)?;

    match (
        title,
        credentials,
        pause_before_playing,
        cd_device,
        usb_device,
        show_buffer,
        http_found,
        file_path_found,
        url_list.as_slice(),
    ) {
        (None, None, None, Some(device), None, None, false, false, []) => {
            Ok(Station::CD { index, device })
        }
        (None, None, None, None, Some(device), None, false, false, []) => Ok(Station::Usb {
            index,
            device,
            shuffle,
        }),
        (title, Some(credentials), None, None, None, show_buffer, false, true, [_]) => {
            Ok(Station::SambaServer {
                index,
                title,
                credentials,
                show_buffer,
                remote_address: url_list.pop().unwrap(),
                shuffle,
            })
        }
        (title, None, pause_before_playing, None, None, show_buffer, true, false, _) => {
            Ok(Station::UrlList {
                index,
                title,
                pause_before_playing,
                show_buffer,
                tracks: url_list
                    .into_iter()
                    .map(|url| Track::url(url.into()))
                    .collect(),
                shuffle,
            })
        }
        _ => Err(ParsePlaylistError::BadPlaylist),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_INDEX: &str = "00";
    const TEST_TITLE: &str = "My Title";
    const TEST_URL: &str = "http://listen.somewhere.com/radio#=?";
    const TEST_USERNAME: &str = "server_username";
    const TEST_PASSWORD: &str = "server_password";
    const TEST_REMOTE_ADDRESS: &str = "//127.0.0.1/server";

    #[test]
    fn correct_cd_device() {
        let device = String::from("/dev/sda");
        let source = format!("cd:{}\n", device);
        let playlist = parse_data(source.as_bytes(), TEST_INDEX.into()).unwrap();
        assert_eq!(
            playlist,
            Station::CD {
                index: TEST_INDEX.into(),
                device
            }
        );
    }

    #[test]
    fn correct_url_list() {
        let source = format!("{}\n", TEST_URL);
        let playlist = parse_data(source.as_bytes(), TEST_INDEX.into()).unwrap();

        assert_eq!(
            playlist,
            Station::UrlList {
                index: TEST_INDEX.into(),
                title: None,
                show_buffer: None,
                pause_before_playing: None,
                tracks: vec![Track::url(TEST_URL.into())],
                shuffle: false,
            }
        );
    }

    #[test]
    fn shuffle_url_list() {
        let source = format!("{}\n#shuffle\n", TEST_URL);
        let playlist = parse_data(source.as_bytes(), TEST_INDEX.into()).unwrap();

        assert_eq!(
            playlist,
            Station::UrlList {
                index: TEST_INDEX.into(),
                title: None,
                show_buffer: None,
                pause_before_playing: None,
                tracks: vec![Track::url(TEST_URL.into())],
                shuffle: true,
            }
        );
    }

    #[test]
    fn url_lists_can_have_titles() {
        let source = format!("#title={}\n{}\n", TEST_TITLE, TEST_URL);
        let playlist = parse_data(source.as_bytes(), TEST_INDEX.into()).unwrap();
        assert_eq!(
            playlist,
            Station::UrlList {
                index: TEST_INDEX.into(),
                title: Some(TEST_TITLE.into()),
                show_buffer: None,
                pause_before_playing: None,
                tracks: vec![Track::url(TEST_URL.into())],
                shuffle: false,
            }
        );
    }

    #[test]
    fn correct_file_server() {
        let source = format!(
            "#username={}\n#password={}\n{}\n",
            TEST_USERNAME, TEST_PASSWORD, TEST_REMOTE_ADDRESS
        );
        let playlist = parse_data(source.as_bytes(), TEST_INDEX.into()).unwrap();
        assert_eq!(
            playlist,
            Station::SambaServer {
                index: TEST_INDEX.into(),
                title: None,
                credentials: Credentials {
                    username: TEST_USERNAME.into(),
                    password: TEST_PASSWORD.into()
                },
                show_buffer: None,
                remote_address: TEST_REMOTE_ADDRESS.into(),
                shuffle: false,
            }
        );
    }

    #[test]
    fn file_servers_can_have_titles() {
        let source = format!(
            "#title={}\n#username={}\n#password={}\n{}\n",
            TEST_TITLE, TEST_USERNAME, TEST_PASSWORD, TEST_REMOTE_ADDRESS
        );
        let playlist = parse_data(source.as_bytes(), TEST_INDEX.into()).unwrap();
        assert_eq!(
            playlist,
            Station::SambaServer {
                index: TEST_INDEX.into(),
                title: Some(TEST_TITLE.into()),
                credentials: Credentials {
                    username: TEST_USERNAME.into(),
                    password: TEST_PASSWORD.into(),
                },
                show_buffer: None,
                remote_address: TEST_REMOTE_ADDRESS.into(),
                shuffle: false,
            }
        );
    }

    #[test]
    fn parameter_casing_doesnt_matter() {
        let source = format!(
            "#TITLE={}\n#username={}\n#password={}\n{}\n",
            TEST_TITLE, TEST_USERNAME, TEST_PASSWORD, TEST_REMOTE_ADDRESS
        );
        let playlist = parse_data(source.as_bytes(), TEST_INDEX.into()).unwrap();
        assert_eq!(
            playlist,
            Station::SambaServer {
                index: TEST_INDEX.into(),
                title: Some(TEST_TITLE.into()),
                credentials: Credentials {
                    username: TEST_USERNAME.into(),
                    password: TEST_PASSWORD.into(),
                },
                show_buffer: None,
                remote_address: TEST_REMOTE_ADDRESS.into(),
                shuffle: false,
            }
        );
    }

    #[test]
    fn invalid_parameters_are_ignored() {
        let source = format!(
            "#title={}\n#bad_parameter=rubbish\n{}\n",
            TEST_TITLE, TEST_URL
        );
        let playlist = parse_data(source.as_bytes(), TEST_INDEX.into()).unwrap();
        assert_eq!(
            playlist,
            Station::UrlList {
                index: TEST_INDEX.into(),
                title: Some(TEST_TITLE.into()),
                show_buffer: None,
                pause_before_playing: None,
                tracks: vec![Track::url(TEST_URL.into())],
                shuffle: false,
            }
        );
    }

    #[test]
    fn empty_playlist_returns_error_message() {
        let source = format!("#title={}\n#bad_parameter=rubbush\n", TEST_TITLE);
        match parse_data(source.as_bytes(), TEST_INDEX.into()).unwrap_err() {
            ParsePlaylistError::EmptyPlaylist => (),
            err => panic!(
                "Got {:?}. We should have got ParsePlaylistError::EmptyPlaylist",
                err
            ),
        }
    }

    #[test]
    fn invalid_contents_produces_an_error() {
        let source = format!("{}\nthis line is invalid\n", TEST_URL);
        match parse_data(source.as_bytes(), TEST_INDEX.into()).unwrap_err() {
            ParsePlaylistError::BadPlaylistLine { line_number, line } => {
                assert_eq!(line_number, 2);
                assert_eq!(line, "this line is invalid".to_string());
            }
            err => panic!(
                "Got {:?}. We should have got ParsePlaylistError::EmptyPlaylist",
                err
            ),
        }
    }
}
