use std::path::PathBuf;
use thiserror::Error;

/// Root domain error for directory scanning and media discovery.
#[derive(Debug, Error)]
pub enum ScanError {
    #[error("Invalid root path: {0}")]
    InvalidPath(PathBuf),

    #[error("File system I/O error on '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Scan session '{0}' not found or already completed")]
    SessionNotFound(uuid::Uuid),

    #[error("Scan session cancelled")]
    Cancelled,

    #[error("Channel communication error: {0}")]
    ChannelClosed(String),
}

/// Categorized non-fatal errors encountered during directory traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum ScanErrorKind {
    PermissionDenied,
    NotFound,
    CircularSymlink,
    IoError,
}

impl std::fmt::Display for ScanErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PermissionDenied => write!(f, "PermissionDenied"),
            Self::NotFound => write!(f, "NotFound"),
            Self::CircularSymlink => write!(f, "CircularSymlink"),
            Self::IoError => write!(f, "IoError"),
        }
    }
}

/// Domain errors during media metadata extraction.
#[derive(Debug, Error)]
pub enum ExtractionError {
    #[error("File not found: '{0}'")]
    FileNotFound(PathBuf),

    #[error("I/O error reading '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Corrupted or malformed header in '{path}': {message}")]
    CorruptHeader { path: PathBuf, message: String },

    #[error("Unsupported media format for extraction: '{0}'")]
    UnsupportedFormat(String),
}

/// Domain errors during media fingerprinting and hash computation.
#[derive(Debug, Error)]
pub enum FingerprintError {
    #[error("File not found: '{0}'")]
    FileNotFound(PathBuf),

    #[error("I/O error reading '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Image decode error on '{path}': {message}")]
    ImageDecode { path: PathBuf, message: String },

    #[error("Unsupported format for fingerprinting: '{0}'")]
    UnsupportedFormat(String),

    #[error("File is empty (0 bytes): '{0}'")]
    ZeroByteFile(PathBuf),
}

/// Domain errors during cluster evaluation and deduplication.
#[derive(Debug, Error)]
pub enum DeduplicationError {
    #[error("Multi-index lookup index corrupted or invalid state")]
    IndexCorrupt,

    #[error("Channel communication error: {0}")]
    ChannelClosed(String),

    #[error("Deduplication processing failed: {0}")]
    ProcessingFailed(String),

    #[error("Fingerprinting error: {0}")]
    Fingerprint(#[from] FingerprintError),
}

/// Domain errors during media metadata persistence and database operations.
#[derive(Debug, Error)]
pub enum StorageError {
    #[error("Database connection failed to {uri}: {message}")]
    ConnectionFailed { uri: String, message: String },

    #[error("Database authentication failed on {database}: {message}")]
    AuthenticationFailed { database: String, message: String },

    #[error("Index initialization failed on collection '{collection}': {message}")]
    IndexCreationFailed { collection: String, message: String },

    #[error("Bulk write failed on collection '{collection}': {message}")]
    BulkWriteFailed {
        collection: String,
        message: String,
        is_transient: bool,
    },

    #[error("Failed to serialize document to BSON: {0}")]
    SerializationError(String),

    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    #[error("Database client configuration error: {0}")]
    ConfigError(String),
}
