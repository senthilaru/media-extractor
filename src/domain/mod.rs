pub mod dedup_models;
pub mod errors;
pub mod metadata_models;
pub mod models;
pub mod ports;

pub use dedup_models::{
    CanonicalAsset, DeduplicationConfig, DeduplicationSummary, DuplicateCluster,
    DuplicateRelationship, DuplicateVariant, FingerprintConfig, MediaFingerprint, RotatedHashes,
};
pub use errors::{DeduplicationError, ExtractionError, FingerprintError, ScanError, ScanErrorKind};
pub use metadata_models::{
    AppProfile, CameraMetadata, DatabaseSinkConfig, ExposureMetadata, ExtractionResultKind,
    ExtractionStatus, FileContext, GeoLocation, ImageDimensions, MediaMetadataDocument,
    TemporalMetadata, TimestampSource, VideoMetadata,
};
pub use models::{
    AtomicSessionCounters, DiscoveredMediaItem, DiscoveryBatch, MediaCategory, ScanErrorRecord,
    ScanSession, SessionMetrics, SessionStatus, SharedSessionCounters,
};
pub use ports::{
    BatchMetadataExtractor, DeduplicationEngine, DirectoryScanner, MediaClassifier,
    MediaFingerprinter, MediaMetadataExtractor, MultiIndexLookup, ScanConfig, ScanStream,
};
