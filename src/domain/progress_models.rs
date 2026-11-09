use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use uuid::Uuid;

/// Active high-level operational stage of pipeline execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationalPhase {
    /// Walking filesystem directory tree and discovering media files
    Discovery,
    /// Extracting container metadata, EXIF, QuickTime, and dimensions
    Extraction,
    /// Computing BLAKE3 cryptographic digests or perceptual DCT visual hashes
    Fingerprinting,
    /// Ingesting documents to MongoDB storage sink or writing output stream
    Persistence,
    /// Completed, queues drained, resources flushed
    Completed,
}

impl std::fmt::Display for OperationalPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Discovery => write!(f, "Discovery"),
            Self::Extraction => write!(f, "Extraction"),
            Self::Fingerprinting => write!(f, "Fingerprinting"),
            Self::Persistence => write!(f, "Persistence"),
            Self::Completed => write!(f, "Completed"),
        }
    }
}

/// High-level performance profile controlling extraction depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, Serialize, Deserialize, Default)]
pub enum ExtractionMode {
    /// Fast metadata-only mode: container headers, EXIF/QuickTime, dimensions, timestamps.
    /// Bypasses full-file bitwise cryptographic hashing and perceptual visual hashing.
    Fast,
    /// Balanced mode: Fast metadata + sample hashing; deep hashing only for files below threshold.
    #[default]
    Balanced,
    /// Full mode: Complete bitwise cryptographic hashing and perceptual deduplication for all files.
    Full,
}

impl std::fmt::Display for ExtractionMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fast => write!(f, "Fast (Metadata-only)"),
            Self::Balanced => write!(f, "Balanced"),
            Self::Full => write!(f, "Full (Deep Hashing)"),
        }
    }
}

/// Configuration settings for extraction performance, concurrency, and hashing limits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionPerformanceProfile {
    /// Performance mode (Fast, Balanced, Full)
    pub mode: ExtractionMode,
    /// File size limit in bytes above which full-file cryptographic hashing is skipped (e.g. 100 MB).
    /// If None, no size limit is applied.
    pub max_hash_size_bytes: Option<u64>,
    /// Whether perceptual visual hashing (pHash) is active
    pub enable_perceptual: bool,
    /// Whether full cryptographic bitwise hashing (BLAKE3) is active
    pub enable_cryptographic_hash: bool,
    /// Dedicated extraction worker pool concurrency
    pub worker_concurrency: usize,
    /// Batch size for discovered items
    pub batch_size: usize,
    /// Channel capacity for bounded memory backpressure
    pub channel_capacity: usize,
    /// Cadence for progress updates in milliseconds
    pub update_interval_ms: u64,
    /// Whether to force re-extraction and bypass incremental pre-filtering
    pub force_reindex: bool,
}

impl Default for ExtractionPerformanceProfile {
    fn default() -> Self {
        Self {
            mode: ExtractionMode::Balanced,
            max_hash_size_bytes: Some(100 * 1024 * 1024), // 100 MB default
            enable_perceptual: true,
            enable_cryptographic_hash: true,
            worker_concurrency: std::thread::available_parallelism()
                .map(|p| p.get())
                .unwrap_or(8),
            batch_size: 500,
            channel_capacity: 100,
            update_interval_ms: 250,
            force_reindex: false,
        }
    }
}

impl ExtractionPerformanceProfile {
    /// Create a fast-mode profile that skips full bitwise hashing and perceptual hashing.
    pub fn fast() -> Self {
        Self {
            mode: ExtractionMode::Fast,
            max_hash_size_bytes: None,
            enable_perceptual: false,
            enable_cryptographic_hash: false,
            worker_concurrency: std::thread::available_parallelism()
                .map(|p| p.get())
                .unwrap_or(8),
            batch_size: 500,
            channel_capacity: 100,
            update_interval_ms: 250,
            force_reindex: false,
        }
    }

    /// Check if full-file cryptographic hashing should be performed for a given file size.
    pub fn should_hash_file(&self, file_size_bytes: u64) -> bool {
        if !self.enable_cryptographic_hash || self.mode == ExtractionMode::Fast {
            return false;
        }
        if let Some(limit) = self.max_hash_size_bytes {
            file_size_bytes <= limit
        } else {
            true
        }
    }
}

/// Instantaneous progress snapshot emitted to standard error or structured consumers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanProgressSnapshot {
    /// Active scan session identifier
    pub session_id: Uuid,
    /// Elapsed wall-clock duration since scan initiation
    pub elapsed_duration: Duration,
    /// Current operational phase
    pub active_phase: OperationalPhase,
    /// Directories visited during traversal
    pub directories_scanned: u64,
    /// Total media files discovered so far
    pub items_discovered: u64,
    /// Total byte volume discovered so far
    pub bytes_discovered: u64,
    /// Items skipped because they already exist in database and are unmodified
    pub items_skipped: u64,
    /// Cumulative byte volume skipped
    pub bytes_skipped: u64,
    /// Total media files extracted
    pub items_processed: u64,
    /// Total byte volume processed
    pub bytes_processed: u64,
    /// Items successfully persisted to database (if configured)
    pub items_persisted: u64,
    /// Instantaneous or moving-average processing throughput in items/second
    pub items_per_sec: f64,
    /// Instantaneous or moving-average data throughput in Megabytes/second
    pub mb_per_sec: f64,
    /// Estimated time remaining to complete processing
    pub estimated_remaining: Option<Duration>,
    /// Number of errors or skipped items encountered
    pub errors_encountered: u64,
    /// Number of extraction warnings encountered
    pub warnings_encountered: u64,
    /// Whether directory discovery has finished enumerating all paths
    pub is_discovery_complete: bool,
}

/// Post-scan audit report produced upon normal completion or signal interruption (Ctrl+C).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionSummary {
    /// Scan session identifier
    pub session_id: Uuid,
    /// Root path scanned
    pub target_path: PathBuf,
    /// Extraction performance mode utilized
    pub mode: ExtractionMode,
    /// Timestamp scan began
    pub started_at: DateTime<Utc>,
    /// Timestamp scan ended
    pub completed_at: DateTime<Utc>,
    /// Total elapsed wall-clock duration
    pub elapsed_duration: Duration,
    /// Total media files discovered
    pub total_discovered: u64,
    /// Total media files skipped because they are already indexed and unmodified
    pub total_skipped: u64,
    /// Total media files extracted
    pub total_processed: u64,
    /// Total data volume processed in bytes
    pub total_bytes_processed: u64,
    /// Total data volume skipped in bytes
    pub total_bytes_skipped: u64,
    /// Overall average throughput in items/second
    pub average_items_per_sec: f64,
    /// Overall average data rate in Megabytes/second
    pub average_mb_per_sec: f64,
    /// Database records upserted
    pub db_upserted: u64,
    /// Database records matched (existing identical records)
    pub db_matched: u64,
    /// Database records modified
    pub db_modified: u64,
    /// Database persistence errors
    pub db_failed: u64,
    /// Extraction or file read errors
    pub extraction_errors: u64,
    /// Extraction warnings (e.g. partial EXIF, corrupt tags)
    pub warnings_count: u64,
    /// Whether scan completed to 100% or was cancelled early
    pub was_cancelled: bool,
    /// Whether force re-index was active
    pub force_reindex: bool,
}
