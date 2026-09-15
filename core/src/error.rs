use thiserror::Error;

/// Crate-wide error type.
#[derive(Debug, Error)]
pub enum Error {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("migration error: {0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),

    #[error("image error: {0}")]
    Image(#[from] image::ImageError),

    #[error("auth error: {0}")]
    Auth(String),

    #[error("invalid username: {0:?}")]
    InvalidUser(String),

    #[error("path traversal rejected: {0:?}")]
    PathTraversal(String),

    /// Model loading, inference, or weight download. Carries the underlying
    /// message rather than wrapping `ort`'s error type, so the public API
    /// doesn't leak the inference backend.
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
