use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
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
    pub hash_90: u64,
    /// DCT pHash for 180° rotated raster
    pub hash_180: u64,
    /// DCT pHash for 270° clockwise rotated raster
    pub hash_270: u64,
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
}

impl Default for FingerprintConfig {
    fn default() -> Self {
        Self {
            enable_perceptual: true,
            enable_rotation: true,
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
