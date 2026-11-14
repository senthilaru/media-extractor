use std::sync::Arc;
use std::time::Instant;
use tracing::{info, instrument};

use crate::domain::audit_models::{
    CatalogAuditReport, CommandRecommendation, InspectionCategory, InspectionSample,
};
use crate::domain::errors::StorageError;
use crate::domain::ports::MetadataStorageSink;

/// Application service that compiles catalog audit reports, evaluates next-action heuristics,
/// and retrieves sample file paths for manual inspection.
#[derive(Clone)]
pub struct AuditService {
    storage_sink: Arc<dyn MetadataStorageSink>,
}

impl AuditService {
    /// Constructs a new `AuditService` with the provided storage sink adapter.
    pub fn new(storage_sink: Arc<dyn MetadataStorageSink>) -> Self {
        Self { storage_sink }
    }

    /// Generates a comprehensive catalog audit report, computes extension percentages,
    /// evaluates the next recommended CLI command, and logs timing telemetry.
    #[instrument(skip(self), name = "generate_audit_report")]
    pub async fn generate_report(&self) -> Result<CatalogAuditReport, StorageError> {
        let start_time = Instant::now();

        let mut report = self.storage_sink.generate_audit_report().await?;

        // Evaluate contextual recommendation heuristic based on catalog metrics
        report.recommended_action = Some(Self::evaluate_recommendation(&report));

        let elapsed = start_time.elapsed();
        info!(
            total_files = report.total_files,
            timetaken_ms = elapsed.as_millis(),
            "Compiled catalog audit report in {:?}",
            elapsed
        );

        Ok(report)
    }

    /// Queries top sample items for an inspection category with a configurable limit (default 3).
    #[instrument(skip(self), name = "inspect_samples")]
    pub async fn inspect_samples(
        &self,
        category: InspectionCategory,
        limit: usize,
    ) -> Result<Vec<InspectionSample>, StorageError> {
        let start_time = Instant::now();
        let samples = self
            .storage_sink
            .query_inspection_samples(category, limit)
            .await?;
        let elapsed = start_time.elapsed();

        info!(
            category = %category,
            sample_count = samples.len(),
            timetaken_ms = elapsed.as_millis(),
            "Retrieved inspection samples in {:?}",
            elapsed
        );

        Ok(samples)
    }

    /// Deterministic priority decision tree that determines the next logical CLI command
    /// based on prioritized catalog state.
    pub fn evaluate_recommendation(report: &CatalogAuditReport) -> CommandRecommendation {
        if report.unmastered_files > 0 {
            CommandRecommendation {
                priority: 1,
                title: "Ingest Unmastered Media Assets".to_string(),
                reason: format!(
                    "{} scanned media assets are waiting to be ingested into canonical master storage",
                    report.unmastered_files
                ),
                command: "cargo run -- ingest".to_string(),
            }
        } else if report.yet_to_replicate > 0
            || report.replica_drives.iter().any(|d| d.pending_count > 0)
        {
            CommandRecommendation {
                priority: 2,
                title: "Synchronize Backup Replicas".to_string(),
                reason: format!(
                    "{} canonical master assets are awaiting replication across backup drives",
                    report.yet_to_replicate
                ),
                command: "cargo run -- sync-replicas".to_string(),
            }
        } else if report.marked_for_delete > 0 {
            CommandRecommendation {
                priority: 3,
                title: "Prune Redundant Duplicate Files".to_string(),
                reason: format!(
                    "{} duplicate media files are marked for safe pruning",
                    report.marked_for_delete
                ),
                command: "cargo run -- organize".to_string(),
            }
        } else if report.problem_files > 0 {
            CommandRecommendation {
                priority: 4,
                title: "Inspect Problematic Assets".to_string(),
                reason: format!(
                    "{} media files encountered extraction or parsing errors and should be checked",
                    report.problem_files
                ),
                command: "cargo run -- report inspect problems".to_string(),
            }
        } else if report.missing_exif > 0 {
            CommandRecommendation {
                priority: 5,
                title: "Review Files Missing EXIF Dates".to_string(),
                reason: format!(
                    "{} media files lack camera/video capture timestamps (using file modification date)",
                    report.missing_exif
                ),
                command: "cargo run -- report inspect missing-exif".to_string(),
            }
        } else {
            CommandRecommendation {
                priority: 6,
                title: "Catalog Fully Synchronized & Healthy".to_string(),
                reason: "All media assets are organized in canonical master, replicated to backup drives, and clean of errors".to_string(),
                command: "cargo run -- report".to_string(),
            }
        }
    }
}
