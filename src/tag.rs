#[derive(Debug)]
pub enum Tag {
    Title(String),
    Artist(String),
    Album(String),
    Genre(String),
    Unknown { name: String, value: String },
}
