//! High-throughput asynchronous directory walker for media discovery
//! with backpressured batch streaming.
//!
//! Architected according to Domain-Driven Design (DDD) and Hexagonal Architecture principles.

pub mod application;
pub mod domain;
pub mod infrastructure;

// Convenience re-exports
pub use application::{
    ExtractionPipeline, MediaDeduplicationEngine, MetadataExtractionEngine, ProgressTracker,
};
pub use domain::dedup_models::{
    CanonicalAsset, DeduplicationConfig, DeduplicationSummary, DuplicateCluster,
    DuplicateRelationship, DuplicateVariant, FingerprintConfig, MediaFingerprint, RotatedHashes,
};
pub use domain::errors::{
    DeduplicationError, ExtractionError, FingerprintError, ScanError, ScanErrorKind, StorageError,
};
pub use domain::metadata_models::{
    AppProfile, CameraMetadata, DatabaseSinkConfig, ExposureMetadata, ExtractionResultKind,
    ExtractionStatus, FileContext, FilterEvaluationResult, GeoLocation, ImageDimensions,
    MediaMetadataDocument, TemporalMetadata, TimestampSource, VideoMetadata,
};
pub use domain::models::{
    DiscoveredMediaItem, DiscoveryBatch, MediaCategory, ScanErrorRecord, ScanSession,
    SessionMetrics, SessionStatus,
};
pub use domain::ports::{
    BatchMetadataExtractor, BatchWriteStats, DeduplicationEngine, DirectoryScanner,
    MediaClassifier, MediaFingerprinter, MediaMetadataExtractor, MetadataStorageSink,
    MultiIndexLookup, ProgressReporter, ProgressSink, ScanConfig, ScanStream,
};
pub use domain::progress_models::{
    ExecutionSummary, ExtractionMode, ExtractionPerformanceProfile, OperationalPhase,
    ScanProgressSnapshot,
};
pub use infrastructure::mongodb_sink::MongoDbStorageSink;
pub use infrastructure::multi_index::MultiIndexLookupTable;
pub use infrastructure::terminal_progress::IndicatifTerminalProgress;
