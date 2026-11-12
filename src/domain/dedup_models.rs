use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

use crate::domain::metadata_models::ImageDimensions;
use crate::domain::models::MediaCategory;

/// Content fingerprint capturing multi-tier cryptographic and visual identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaFingerprint {
    /// Unique identifier for this fingerprint record
    pub id: Uuid,

    /// Filesystem path to the media file
    pub file_path: PathBuf,

    /// Total byte length reported by filesystem (>= 0)
    pub file_size_bytes: u64,

    /// Media category (`Photo`, `Video`, `RawPhoto`)
    pub media_category: MediaCategory,

    /// Dimensions if known
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<ImageDimensions>,

    /// Tier 2 fast sample digest (first 16 KB + last 16 KB + size)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_digest: Option<String>,

    /// Tier 3 full streaming cryptographic digest (BLAKE3 64 hex characters)
    pub cryptographic_hash: String,

    /// Tier 4 64-bit DCT perceptual hash for canonical 0° orientation
    #[serde(
        rename = "phash",
        alias = "perceptual_hash",
        default,
        deserialize_with = "deserialize_u64_or_signed_opt",
        skip_serializing_if = "Option::is_none"
    )]
    pub perceptual_hash: Option<u64>,

    /// Hexadecimal string representation of the 64-bit DCT perceptual hash
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phash_hex: Option<String>,

    /// Rotated perceptual hashes for 90°, 180°, and 270° orientations
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotated_hashes: Option<RotatedHashes>,

    /// True if image is blank, monochromatic, or low-entropy
    pub is_low_entropy: bool,

    /// UTC timestamp when fingerprint was computed
    pub fingerprinted_at: DateTime<Utc>,
}

impl MediaFingerprint {
    /// Computes the minimum Hamming distance against another fingerprint across all 4 orientations.
    pub fn min_hamming_distance(&self, other: &Self) -> Option<u32> {
        let h1 = self.perceptual_hash?;
        let h2 = other.perceptual_hash?;

        let mut min_dist = (h1 ^ h2).count_ones();

        if let Some(ref rot) = other.rotated_hashes {
            min_dist = min_dist
                .min((h1 ^ rot.hash_90).count_ones())
                .min((h1 ^ rot.hash_180).count_ones())
                .min((h1 ^ rot.hash_270).count_ones());
        }

        if let Some(ref rot) = self.rotated_hashes {
            min_dist = min_dist
                .min((rot.hash_90 ^ h2).count_ones())
                .min((rot.hash_180 ^ h2).count_ones())
                .min((rot.hash_270 ^ h2).count_ones());
        }

        Some(min_dist)
    }
}

/// Rotated perceptual visual hashes for orientation invariance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RotatedHashes {
    /// DCT pHash for 90° clockwise rotated raster
    #[serde(deserialize_with = "deserialize_u64_or_signed")]
    pub hash_90: u64,
    /// DCT pHash for 180° rotated raster
    #[serde(deserialize_with = "deserialize_u64_or_signed")]
    pub hash_180: u64,
    /// DCT pHash for 270° clockwise rotated raster
    #[serde(deserialize_with = "deserialize_u64_or_signed")]
    pub hash_270: u64,
}

/// Custom deserializer that accepts either an unsigned u64 or a two's-complement signed i64.
/// Needed because BSON and some JSON serializers store 64-bit hashes as signed 64-bit integers.
pub fn deserialize_u64_or_signed<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct U64OrSignedVisitor;

    impl<'de> serde::de::Visitor<'de> for U64OrSignedVisitor {
        type Value = u64;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an unsigned 64-bit integer or signed 64-bit integer")
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(value)
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(value as u64)
        }

        fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(value as u64)
        }

        fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            if let Ok(v) = value.parse::<u64>() {
                Ok(v)
            } else if let Ok(v) = value.parse::<i64>() {
                Ok(v as u64)
            } else {
                Err(E::custom(format!("invalid u64 string: {}", value)))
            }
        }
    }

    deserializer.deserialize_any(U64OrSignedVisitor)
}

/// Deserializer for Option<u64> that accepts None/null, unsigned u64, or two's-complement signed i64.
pub fn deserialize_u64_or_signed_opt<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct OptU64Visitor;

    impl<'de> serde::de::Visitor<'de> for OptU64Visitor {
        type Value = Option<u64>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an optional unsigned 64-bit integer or signed 64-bit integer")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_some<D2>(self, deserializer: D2) -> Result<Self::Value, D2::Error>
        where
            D2: serde::Deserializer<'de>,
        {
            deserialize_u64_or_signed(deserializer).map(Some)
        }

        fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(value))
        }

        fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(value as u64))
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }
    }

    deserializer.deserialize_option(OptU64Visitor)
}

/// Category of relationship between duplicate items in a cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DuplicateRelationship {
    /// 100% bitwise identical byte content (Tier 3 match)
    ExactBitwise,
    /// Same visual scene within perceptual threshold
    PerceptualVisualDuplicate,
    /// Downscaled lower-resolution copy of canonical master
    ThumbnailVariant,
    /// Recompressed copy saved with different encoding/quality
    RecompressedVariant,
}

impl std::fmt::Display for DuplicateRelationship {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ExactBitwise => write!(f, "ExactBitwise"),
            Self::PerceptualVisualDuplicate => write!(f, "PerceptualVisualDuplicate"),
            Self::ThumbnailVariant => write!(f, "ThumbnailVariant"),
            Self::RecompressedVariant => write!(f, "RecompressedVariant"),
        }
    }
}

/// Selected primary master file for a duplicate cluster.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanonicalAsset {
    pub file_path: PathBuf,
    pub file_size_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<ImageDimensions>,
    pub media_category: MediaCategory,
    pub quality_score: f64,
}

/// Redundant or variant file in a duplicate cluster.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DuplicateVariant {
    pub file_path: PathBuf,
    pub file_size_bytes: u64,
    pub relationship: DuplicateRelationship,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hamming_distance: Option<u32>,
    pub similarity_score: f64,
    pub reclaimable_bytes: u64,
}

/// Group of duplicate media files containing one canonical master and variant files.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DuplicateCluster {
    pub cluster_id: Uuid,
    pub relationship_type: DuplicateRelationship,
    pub canonical_asset: CanonicalAsset,
    pub variants: Vec<DuplicateVariant>,
    pub total_reclaimable_bytes: u64,
    pub total_files: usize,
}

/// Aggregated dataset-level summary of deduplication run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeduplicationSummary {
    pub total_files_scanned: usize,
    pub unique_media_count: usize,
    pub duplicate_clusters_count: usize,
    pub exact_duplicate_files: usize,
    pub near_duplicate_files: usize,
    pub total_reclaimable_bytes: u64,
    pub tier_pruned_percentage: f64,
    pub elapsed_seconds: f64,
}

/// Configuration options for fingerprint generation.
#[derive(Debug, Clone)]
pub struct FingerprintConfig {
    pub enable_perceptual: bool,
    pub enable_rotation: bool,
    pub enable_cryptographic_hash: bool,
    pub max_hash_size_bytes: Option<u64>,
}

impl Default for FingerprintConfig {
    fn default() -> Self {
        Self {
            enable_perceptual: true,
            enable_rotation: true,
            enable_cryptographic_hash: true,
            max_hash_size_bytes: Some(100 * 1024 * 1024),
        }
    }
}

/// Configuration options for cluster evaluation.
#[derive(Debug, Clone)]
pub struct DeduplicationConfig {
    /// Maximum Hamming distance to consider near-duplicate (default: 10)
    pub similarity_threshold: u32,
    /// Whether to evaluate perceptual near-duplicates (default: true)
    pub enable_perceptual: bool,
    /// Fast prune threshold for Tier 1 size groups
    pub min_cluster_size: usize,
}

impl Default for DeduplicationConfig {
    fn default() -> Self {
        Self {
            similarity_threshold: 10,
            enable_perceptual: true,
            min_cluster_size: 2,
        }
    }
}

/// Specification of a drive group tier.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriveGroupSpec {
    pub priority: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// Physical drive definition mapped to a group role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriveDefinition {
    pub id: String,
    pub group: String,
    pub mount_path: PathBuf,
    pub master_path: PathBuf,
}

/// Custom rules for canonical path validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriveRules {
    #[serde(default = "default_category_folders")]
    pub category_folders: Vec<String>,
    #[serde(default = "default_date_patterns")]
    pub date_patterns: Vec<String>,
}

fn default_category_folders() -> Vec<String> {
    vec![
        "Videos".to_string(),
        "Photos".to_string(),
        "PhotosRaw".to_string(),
    ]
}

fn default_date_patterns() -> Vec<String> {
    vec!["%d-%b-%Y".to_string(), "%Y-%m-%d".to_string()]
}

impl Default for DriveRules {
    fn default() -> Self {
        Self {
            category_folders: default_category_folders(),
            date_patterns: default_date_patterns(),
        }
    }
}

/// Complete configuration parsed from drives.yaml.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriveConfiguration {
    pub version: String,
    pub groups: HashMap<String, DriveGroupSpec>,
    pub drives: Vec<DriveDefinition>,
    #[serde(default)]
    pub rules: DriveRules,
}

/// Evaluated lifecycle status of an asset within a duplicate cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeduplicationStatus {
    CanonicalMaster,
    ReplicaKeep,
    MarkForRemoval,
    NeedsIngestToMaster,
    ConflictReview,
    Removed,
}

impl std::fmt::Display for DeduplicationStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CanonicalMaster => write!(f, "CanonicalMaster"),
            Self::ReplicaKeep => write!(f, "ReplicaKeep"),
            Self::MarkForRemoval => write!(f, "MarkForRemoval"),
            Self::NeedsIngestToMaster => write!(f, "NeedsIngestToMaster"),
            Self::ConflictReview => write!(f, "ConflictReview"),
            Self::Removed => write!(f, "Removed"),
        }
    }
}

/// Deduplication metadata embedded inside MediaMetadataDocument in MongoDB.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeduplicationMetadata {
    pub cluster_id: Uuid,
    pub status: DeduplicationStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_master_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_master_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drive_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drive_group: Option<String>,
    pub is_in_master_path: bool,
    pub reclaimable_bytes: u64,
    pub evaluated_at: DateTime<Utc>,
}

/// Canonical path matching evaluation result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CanonicalPathStatus {
    InMaster {
        year: i32,
        date_folder: String,
        category_folder: String,
    },
    OutsideMaster,
    UnknownDrive,
}

/// Single file removal task in a drive manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriveRemovalTask {
    pub document_id: Uuid,
    pub target_file_path: PathBuf,
    pub canonical_master_path: PathBuf,
    pub file_size_bytes: u64,
    pub master_exists_verified: bool,
}

/// Per-drive removal manifest for guided cleanup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriveRemovalManifest {
    pub drive_id: String,
    pub mount_path: PathBuf,
    pub drive_group: String,
    pub total_files_to_remove: usize,
    pub total_reclaimable_bytes: u64,
    pub removal_tasks: Vec<DriveRemovalTask>,
}

/// Lifecycle status of an individual file ingest task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IngestTaskStatus {
    Pending,
    CopiedVerified,
    SkippedIdentical,
    Failed(String),
}

/// A single file ingestion operation from source drive to primary master archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestTask {
    pub source_doc_id: Uuid,
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub category: MediaCategory,
    pub file_size_bytes: u64,
    pub cryptographic_hash: Option<String>,
    pub status: IngestTaskStatus,
}

/// Comprehensive plan detailing planned file transfers into the primary master structure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MasterIngestPlan {
    pub source_drive_id: String,
    pub target_drive_id: String,
    pub source_mount: PathBuf,
    pub target_master_root: PathBuf,
    pub tasks: Vec<IngestTask>,
    pub total_bytes: u64,
    pub available_target_bytes: u64,
}

/// Runtime configuration options for an ingestion execution.
#[derive(Debug, Clone)]
pub struct IngestOptions {
    pub source_drive: String,
    pub target_drive: Option<String>,
    pub dry_run: bool,
    pub delete_source: bool,
    pub verify_hash: bool,
}

/// Summary report returned upon completion of an ingestion run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngestExecutionReport {
    pub source_drive_id: String,
    pub target_drive_id: String,
    pub is_dry_run: bool,
    pub total_files_planned: usize,
    pub files_transferred: usize,
    pub files_skipped_identical: usize,
    pub files_failed: usize,
    pub bytes_transferred: u64,
    pub source_files_deleted: usize,
    pub elapsed_seconds: f64,
    pub errors: Vec<String>,
}

/// Result of evaluating a target path for file placement and collision resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathResolution {
    /// Target path is unoccupied and available
    Available(PathBuf),
    /// Target path exists and has identical byte length and cryptographic hash
    IdenticalExists(PathBuf),
    /// Target path exists with different content; non-colliding suffix appended
    CollisionResolved(PathBuf),
}

/// A single file replication task from primary master to a secondary replica drive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicaSyncTask {
    pub task_id: Uuid,
    pub canonical_master_id: Uuid,
    pub source_path: PathBuf,
    pub target_path: PathBuf,
    pub replica_drive_id: String,
    pub replica_group: String,
    pub canonical_rel_path: PathBuf,
    pub file_size_bytes: u64,
    pub cryptographic_hash: Option<String>,
    pub status: IngestTaskStatus,
}

/// Execution plan for synchronizing master assets to mounted replica drives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicaSyncPlan {
    pub primary_drive_id: String,
    pub primary_master_root: PathBuf,
    pub active_replica_ids: Vec<String>,
    pub offline_replica_ids: Vec<String>,
    pub tasks: Vec<ReplicaSyncTask>,
    pub total_bytes: u64,
}

/// Execution report for replica synchronization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplicaSyncReport {
    pub primary_drive_id: String,
    pub is_dry_run: bool,
    pub synchronized_replicas: Vec<String>,
    pub offline_replicas: Vec<String>,
    pub total_files_planned: usize,
    pub files_replicated: usize,
    pub files_skipped_identical: usize,
    pub files_failed: usize,
    pub bytes_transferred: u64,
    pub elapsed_seconds: f64,
    pub errors: Vec<String>,
}
