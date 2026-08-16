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
}

pub type Result<T> = std::result::Result<T, Error>;
