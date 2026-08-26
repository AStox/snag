use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum SnagError {
    #[error("{0}")]
    Message(String),
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("image: {0}")]
    Image(#[from] image::ImageError),
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

impl Serialize for SnagError {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl From<String> for SnagError {
    fn from(value: String) -> Self {
        Self::Message(value)
    }
}

impl From<&str> for SnagError {
    fn from(value: &str) -> Self {
        Self::Message(value.into())
    }
}

pub type Result<T> = std::result::Result<T, SnagError>;
