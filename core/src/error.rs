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

    /// The filename a client offered could not be made safe to place on disk.
    #[error("unusable filename: {0}")]
    InvalidFilename(String),

    /// An extension this app does not index. Refused at `init` rather than at
    /// `finish`, so nobody uploads a gigabyte before being told.
    #[error("unsupported file type: {0:?}")]
    UnsupportedMedia(String),

    /// No such upload session — unknown id, or one that expired or belongs to
    /// somebody else. Deliberately one error for all three: distinguishing them
    /// would confirm to a guesser which ids exist.
    #[error("no such upload")]
    UploadNotFound,

    /// A chunk arrived somewhere other than where the file currently ends. The
    /// expected offset comes back so a client that lost track can resync
    /// instead of restarting the file.
    #[error("chunk offset mismatch: expected {expected}")]
    OffsetMismatch { expected: u64 },

    /// The assembled bytes did not hash to what the client promised. The upload
    /// is discarded: a file that is not what it claims to be has no business
    /// being indexed, whether the cause is corruption or a lie.
    #[error("uploaded data does not match the declared hash")]
    HashMismatch,

    /// This user's ceiling would be exceeded. Carries the numbers so the UI can
    /// say how much room is left rather than just refusing.
    #[error("quota exceeded: {used} of {limit} bytes used, {needed} more requested")]
    QuotaExceeded { used: u64, limit: u64, needed: u64 },

    /// Free space would drop under the configured reserve. Distinct from a
    /// quota: nothing the *user* deletes may fix it, and it is the operator who
    /// needs to know.
    #[error("not enough free disk space")]
    DiskFull,

    /// Model loading, inference, or weight download. Carries the underlying
    /// message rather than wrapping `ort`'s error type, so the public API
    /// doesn't leak the inference backend.
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
