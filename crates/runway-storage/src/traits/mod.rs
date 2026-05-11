pub mod document;
pub mod embedding;
pub mod event;
pub mod object;
pub mod vector;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("database error: {0}")]
    Database(String),
    #[error("serialisation error: {0}")]
    Serialisation(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
