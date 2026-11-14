use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::str::FromStr;
use uuid::Uuid;

/// Distribution of media files grouped by file extension.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtensionCount {
    /// Normalized lowercase extension (e.g. "jpg", "heic", "mp4") or "[unknown]"
    pub extension: String,
    /// Number of cataloged assets with this extension
    pub count: u64,
    /// Percentage of total catalog volume represented by this format
    pub percentage: f64,
}

/// Replicated file counts itemized per replica drive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplicaDriveCount {
    /// Identifier of the replica drive (e.g. "replica_1", "replica_2")
    pub drive_id: String,
    /// Number of verified replica backup files cataloged on this drive
    pub replicated_count: u64,
    /// Number of canonical master files yet to be replicated to this drive
    pub pending_count: u64,
}

/// Target status slice for manual verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum InspectionCategory {
    Problems,
    MissingExif,
    Duplicates,
    YetToReplicate,
    MarkedForDelete,
    Unmastered,
    Replicas,
}

impl std::fmt::Display for InspectionCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Problems => write!(f, "problems"),
            Self::MissingExif => write!(f, "missing-exif"),
            Self::Duplicates => write!(f, "duplicates"),
            Self::YetToReplicate => write!(f, "unreplicated"),
            Self::MarkedForDelete => write!(f, "marked-for-delete"),
            Self::Unmastered => write!(f, "unmastered"),
            Self::Replicas => write!(f, "replicas"),
        }
    }
}

impl FromStr for InspectionCategory {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().trim() {
            "problems" | "problem" | "errors" | "failed" => Ok(Self::Problems),
            "missing-exif" | "missing_exif" | "no-exif" | "no_exif" => Ok(Self::MissingExif),
            "duplicates" | "duplicate" | "dups" => Ok(Self::Duplicates),
            "yet-to-replicate" | "unreplicated" | "pending-replicas" | "pending_replicas" => {
                Ok(Self::YetToReplicate)
            }
            "marked-for-delete" | "marked_for_delete" | "prune" | "delete" => {
                Ok(Self::MarkedForDelete)
            }
            "unmastered" | "needs-ingest" | "ingest" => Ok(Self::Unmastered),
            "replicas" | "replica" | "replicated" | "replica-keep" | "replica_keep" => {
                Ok(Self::Replicas)
            }
            other => Err(format!(
                "Unknown inspection category '{}'. Valid categories: problems, missing-exif, duplicates, unreplicated, marked-for-delete, unmastered, replicas",
                other
            )),
        }
    }
}

/// Actionable instruction advising the operator on the next operational pipeline command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRecommendation {
    /// Priority level (1 = highest urgency)
    pub priority: u8,
    /// Short title describing the recommendation
    pub title: String,
    /// Human-readable explanation of why this action is recommended
    pub reason: String,
    /// Exact copy-pasteable CLI command string
    pub command: String,
}

/// Lightweight sample asset retrieved for operator inspection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InspectionSample {
    /// Database document UUID
    pub id: Uuid,
    /// Full filesystem path on disk
    pub file_path: PathBuf,
    /// File size in bytes
    pub file_size_bytes: u64,
    /// Normalized extension
    pub extension: String,
    /// The category under which this sample was retrieved
    pub category: InspectionCategory,
    /// Diagnostic details (error message, replica discrepancies, or master link)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostic_detail: Option<String>,
}

/// Aggregated inventory and health report for the media database catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CatalogAuditReport {
    /// Total number of media documents cataloged in database
    pub total_files: u64,
    /// Breakdown of files grouped by format extension
    pub extension_counts: Vec<ExtensionCount>,
    /// Number of assets recognized as canonical masters
    pub copied_to_master: u64,
    /// Total number of verified replica backup copies preserved on backup drives
    pub total_replicas: u64,
    /// Itemized breakdown of replicated assets per replica drive
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replica_drives: Vec<ReplicaDriveCount>,
    /// Number of canonical masters pending replication across backup drives
    pub yet_to_replicate: u64,
    /// Number of duplicate assets identified for pruning (MarkForRemoval / Removed)
    pub duplicates_detected: u64,
    /// Number of assets marked for deletion/prune
    pub marked_for_delete: u64,
    /// Number of assets lacking EXIF/video capture dates
    pub missing_exif: u64,
    /// Number of assets flagged with extraction or validation problems
    pub problem_files: u64,
    /// Number of scanned assets awaiting master ingestion
    pub unmastered_files: u64,
    /// UTC timestamp when report was compiled
    pub generated_at: DateTime<Utc>,
    /// Highest-priority recommended next CLI action
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommended_action: Option<CommandRecommendation>,
    /// Optional sample items if inspection was requested
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub samples: Option<Vec<InspectionSample>>,
}
