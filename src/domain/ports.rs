use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::mpsc;

use crate::domain::dedup_models::{
    DeduplicationConfig, DeduplicationSummary, DuplicateCluster, FingerprintConfig,
    MediaFingerprint,
};
use crate::domain::errors::{
    DeduplicationError, ExtractionError, FingerprintError, ScanError, StorageError,
};
use crate::domain::metadata_models::{FilterEvaluationResult, MediaMetadataDocument};
use crate::domain::models::{
    DiscoveredMediaItem, DiscoveryBatch, MediaCategory, ScanErrorRecord, ScanSession,
    SessionMetrics,
};
use crate::domain::progress_models::{ExecutionSummary, OperationalPhase, ScanProgressSnapshot};

/// Configuration options for a directory scan run.
#[derive(Debug, Clone)]
pub struct ScanConfig {
    /// Root paths to traverse.
    pub root_paths: Vec<PathBuf>,
    /// Number of media items grouped per batch (default: 500).
    pub batch_size: usize,
    /// Bounded capacity for the batch channel (default: 100 batches).
    pub channel_capacity: usize,
    /// Whether to skip hidden files and directories (default: true).
    pub skip_hidden: bool,
    /// Whether to follow directory symlinks (default: false).
    pub follow_symlinks: bool,
    /// Interval for emitting telemetry heartbeats (default: 1000ms).
    pub telemetry_interval: Duration,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            root_paths: Vec::new(),
            batch_size: 500,
            channel_capacity: 100,
            skip_hidden: true,
            follow_symlinks: false,
            telemetry_interval: Duration::from_millis(1000),
        }
    }
}

/// Output channels produced by an active scan execution.
pub struct ScanStream {
    /// Channel delivering bounded batches of discovered media items.
    pub batches: mpsc::Receiver<DiscoveryBatch>,
    /// Channel delivering non-fatal error records (permission denied, cycles).
    pub errors: mpsc::Receiver<ScanErrorRecord>,
    /// Telemetry channel emitting periodic performance snapshots.
    pub telemetry: mpsc::Receiver<SessionMetrics>,
}

/// Core port for directory traversal engines.
#[async_trait::async_trait]
pub trait DirectoryScanner: Send + Sync {
    /// Start an asynchronous directory scan session according to the provided configuration.
    async fn start_scan(&self, config: ScanConfig) -> Result<(ScanSession, ScanStream), ScanError>;

    /// Request cancellation of an in-progress scan session.
    async fn cancel_scan(&self, session_id: &uuid::Uuid) -> Result<(), ScanError>;
}

/// Port for classifying file paths into media categories without file system I/O.
pub trait MediaClassifier: Send + Sync {
    /// Classifies a path based on its extension or characteristics.
    fn classify(&self, path: &Path) -> Option<MediaCategory>;

    /// Returns whether the provided extension is a supported media format.
    fn is_supported_extension(&self, extension: &str) -> bool;
}

/// Core port for extracting metadata from a single media file.
pub trait MediaMetadataExtractor: Send + Sync {
    /// Extracts rich metadata from a single media file.
    ///
    /// Accepts the discovered media item (path, size, category) and optional scanned root directory.
    fn extract_file(
        &self,
        item: &DiscoveredMediaItem,
        scanned_root: Option<&Path>,
    ) -> Result<MediaMetadataDocument, ExtractionError>;
}

/// Port for batch metadata extraction orchestrators.
#[async_trait::async_trait]
pub trait BatchMetadataExtractor: Send + Sync {
    /// Processes a batch of discovered items in parallel across CPU worker threads,
    /// returning normalized metadata documents.
    async fn extract_batch(
        &self,
        batch: DiscoveryBatch,
        scanned_root: Option<&Path>,
    ) -> Vec<MediaMetadataDocument>;
}

/// Core port for computing cryptographic and perceptual content fingerprints.
pub trait MediaFingerprinter: Send + Sync {
    /// Computes full fingerprint (BLAKE3 + DCT pHash) for a single media item.
    fn fingerprint_file(
        &self,
        item: &DiscoveredMediaItem,
        metadata: Option<&MediaMetadataDocument>,
        config: &FingerprintConfig,
    ) -> Result<MediaFingerprint, FingerprintError>;

    /// Computes Tier 2 fast sample digest (header + trailer sample) without reading full file.
    fn compute_sample_digest(&self, path: &Path) -> Result<String, FingerprintError>;

    /// Computes Tier 3 exact streaming cryptographic digest across file payload.
    fn compute_cryptographic_hash(&self, path: &Path) -> Result<String, FingerprintError>;

    /// Computes Tier 4 64-bit DCT perceptual hash on an image or video preview.
    fn compute_perceptual_hash(&self, path: &Path) -> Result<Option<u64>, FingerprintError>;
}

/// Port for orchestrating the multi-tier deduplication pipeline.
#[async_trait::async_trait]
pub trait DeduplicationEngine: Send + Sync {
    /// Ingests fingerprints and groups them into duplicate clusters.
    async fn find_duplicates(
        &self,
        fingerprints: Vec<MediaFingerprint>,
        config: DeduplicationConfig,
    ) -> Result<(Vec<DuplicateCluster>, DeduplicationSummary), DeduplicationError>;
}

/// Port for sub-linear near-duplicate candidate retrieval using Multi-Index Hashing.
pub trait MultiIndexLookup: Send + Sync {
    /// Inserts a 64-bit perceptual hash with associated item identifier.
    fn insert(&mut self, id: uuid::Uuid, hash: u64);

    /// Finds candidate identifiers within Hamming distance <= threshold.
    fn query_candidates(&self, hash: u64, max_distance: u32) -> Vec<(uuid::Uuid, u32)>;
}

/// Result statistics returned by storage batch persistence operations.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BatchWriteStats {
    /// Number of existing records matched by selector.
    pub matched_count: u64,
    /// Number of existing records updated.
    pub modified_count: u64,
    /// Number of newly inserted/upserted records.
    pub upserted_count: u64,
    /// Number of documents that failed to write.
    pub failed_count: u64,
}

/// Port for asynchronous media metadata document persistence sinks.
#[async_trait::async_trait]
pub trait MetadataStorageSink: Send + Sync {
    /// Validates database reachability and credentials.
    async fn ping(&self) -> Result<(), StorageError>;

    /// Ensures required query and uniqueness indexes exist on the target collection.
    async fn ensure_indexes(&self) -> Result<(), StorageError>;

    /// Persists a batch of metadata documents using idempotent bulk upsert semantics.
    async fn save_batch(
        &self,
        docs: &[MediaMetadataDocument],
    ) -> Result<BatchWriteStats, StorageError>;

    /// Flushes buffered operations and closes connection pools.
    async fn flush(&self) -> Result<(), StorageError>;

    /// Evaluates discovered media items against the database index to filter out already indexed, unmodified items.
    async fn filter_unmodified_items(
        &self,
        items: &[DiscoveredMediaItem],
    ) -> Result<FilterEvaluationResult, StorageError>;
}

/// Port for reporting live progress updates and phase transitions.
pub trait ProgressReporter: Send + Sync {
    /// Update the current operational phase.
    fn set_phase(&self, phase: OperationalPhase);

    /// Record newly discovered items and cumulative byte volume.
    fn record_discovered(&self, count: u64, bytes: u64);

    /// Mark that directory traversal discovery has finished.
    fn set_discovery_complete(&self);

    /// Record extracted/processed items and byte volume.
    fn record_processed(&self, count: u64, bytes: u64);

    /// Record items skipped because they are already indexed and unmodified.
    fn record_skipped(&self, count: u64, bytes: u64);

    /// Record items persisted to storage.
    fn record_persisted(&self, count: u64);

    /// Record extraction error or skip.
    fn record_error(&self);

    /// Record extraction warning.
    fn record_warning(&self);

    /// Get current progress snapshot.
    fn snapshot(&self) -> ScanProgressSnapshot;

    /// Finish tracking and produce execution summary.
    fn finish(&self, cancelled: bool) -> ExecutionSummary;
}

/// Port for consuming progress updates and rendering or transmitting them.
pub trait ProgressSink: Send + Sync {
    /// Render or emit a progress snapshot.
    fn render_progress(&self, snapshot: &ScanProgressSnapshot);

    /// Emit the final execution summary report.
    fn render_summary(&self, summary: &ExecutionSummary);
}
