pub mod audit_models;
pub mod dedup_models;
pub mod errors;
pub mod metadata_models;
pub mod models;
pub mod person_cluster;
pub mod ports;
pub mod progress_models;
pub mod smart_album;
pub mod visual_models;

pub use audit_models::{
    CatalogAuditReport, CommandRecommendation, ExtensionCount, InspectionCategory,
    InspectionSample, ReplicaDriveCount,
};
pub use dedup_models::{
    CanonicalAsset, DeduplicationConfig, DeduplicationSummary, DuplicateCluster,
    DuplicateRelationship, DuplicateVariant, FingerprintConfig, MediaFingerprint, RotatedHashes,
};
pub use person_cluster::{PersonCluster, FACE_CLUSTER_THRESHOLD_COSINE_DISTANCE};
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
    MediaFingerprinter, MediaMetadataExtractor, MultiIndexLookup, ProgressReporter, ProgressSink,
    ScanConfig, ScanStream,
};
pub use progress_models::{
    ExecutionSummary, ExtractionMode, ExtractionPerformanceProfile, OperationalPhase,
    ScanProgressSnapshot,
};
pub use smart_album::{SmartAlbum, SmartAlbumCriteria};
pub use visual_models::{
    BoundingBox, DetectedFace, DetectedObject, DetectedOccasion, ExtractedTextSnippet,
    FacialExpression, VisualSemanticMetadata,
};
