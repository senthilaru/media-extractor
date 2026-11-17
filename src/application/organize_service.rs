use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

use crate::application::dedup_engine::MediaDeduplicationEngine;
use crate::application::drive_pruner::{DrivePrunerOptions, DrivePruningService};
use crate::application::engine::ScanEngine;
use crate::application::ingest_service::MasterIngestService;
use crate::application::metadata_engine::MetadataExtractionEngine;
use crate::application::pipeline::ExtractionPipeline;
use crate::application::replica_service::{ReplicaSyncOptions, ReplicaSyncService};
use crate::domain::dedup_models::{DriveConfiguration, IngestOptions, TransferBufferConfig};
use crate::domain::errors::DeduplicationError;
use crate::domain::ports::{MetadataStorageSink, ProgressSink};
use crate::domain::progress_models::{ExtractionMode, ExtractionPerformanceProfile};
use crate::infrastructure::archive_guard::ArchiveProtectionGuard;
use crate::infrastructure::terminal_progress::IndicatifTerminalProgress;

/// Configuration options for the end-to-end organize pipeline.
#[derive(Debug, Clone)]
pub struct OrganizePipelineOptions {
    /// Optional path to unorganized source directory or drive to organize (if omitted, organizes all pending assets in catalog)
    pub source_path: Option<PathBuf>,
    /// Whether to run in preview/dry-run mode without modifying disk
    pub dry_run: bool,
    /// Ingest and replicate without pruning source files
    pub skip_prune: bool,
    /// Skip destination BLAKE3 verification during copy
    pub no_verify_hash: bool,
    /// I/O buffer size (e.g., "8M")
    pub buffer_size: String,
    /// Force secondary disk re-read pass
    pub recheck_disk_hash: bool,
    /// Force reindex of metadata during extraction
    pub force_reindex: bool,
    /// Batch size for extraction
    pub batch_size: usize,
    /// Channel capacity
    pub channel_capacity: usize,
    /// Suppress non-essential telemetry
    pub quiet: bool,
    /// Keep the source directory itself even if all files are ingested and it becomes empty (defaults to false)
    pub keep_source_dir: bool,
}

/// Comprehensive execution report for the full organize pipeline.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OrganizePipelineReport {
    pub source_path: Option<PathBuf>,
    pub is_dry_run: bool,
    pub files_scanned: usize,
    pub files_extracted: usize,
    pub canonical_masters_count: usize,
    pub replicas_preserved: usize,
    pub files_ingested: usize,
    pub files_replicated: usize,
    pub files_pruned: usize,
    pub empty_directories_removed: usize,
    pub bytes_ingested: u64,
    pub bytes_replicated: u64,
    pub bytes_reclaimed: u64,
    pub total_elapsed_seconds: f64,
    pub stage_durations: Vec<(String, f64)>,
    pub errors: Vec<String>,
}

/// Service orchestrating the complete end-to-end media archival lifecycle.
#[derive(Debug, Default, Clone)]
pub struct OrganizePipelineService;

impl OrganizePipelineService {
    pub fn new() -> Self {
        Self
    }

    /// Executes the full 5-stage media organization pipeline.
    pub async fn execute(
        &self,
        sink: &Arc<dyn MetadataStorageSink>,
        config: &DriveConfiguration,
        options: OrganizePipelineOptions,
    ) -> Result<OrganizePipelineReport, DeduplicationError> {
        let total_start = Instant::now();
        let guard = ArchiveProtectionGuard::from_config(config);

        // Normalize any spurious shell backslash escapes in source_path
        let cleaned_source = options
            .source_path
            .as_ref()
            .map(|p| ArchiveProtectionGuard::clean_escaped_path(p));

        // Fail-fast guard: If source path is provided, it MUST NEVER be inside a protected master archive
        if let Some(ref source) = cleaned_source {
            if guard.is_protected_path(source) {
                return Err(DeduplicationError::PruneForbiddenOnProtectedArchive {
                    drive: "source".to_string(),
                    path: source.clone(),
                    reason: "Source directory cannot reside inside protected canonical master archive."
                        .to_string(),
                });
            }

            if !source.exists() {
                return Err(DeduplicationError::DriveNotMounted {
                    drive: source.to_string_lossy().to_string(),
                    path: source.clone(),
                });
            }
        }

        let mut stage_durations = Vec::new();
        let mut all_errors = Vec::new();

        // =========================================================================
        // STAGE 1: Extract Metadata from Source into MongoDB
        // =========================================================================
        let (files_scanned, files_extracted) = if let Some(ref source) = cleaned_source {
            if !options.quiet {
                eprintln!("\n>>> Stage [1/5]: Extracting Metadata from Source Directory...");
            }
            let stage1_start = Instant::now();

            let profile = ExtractionPerformanceProfile {
                mode: ExtractionMode::Balanced,
                max_hash_size_bytes: Some(100 * 1024 * 1024),
                enable_perceptual: true,
                enable_cryptographic_hash: true,
                worker_concurrency: std::thread::available_parallelism()
                    .map(|p| p.get())
                    .unwrap_or(8),
                batch_size: options.batch_size,
                channel_capacity: options.channel_capacity,
                update_interval_ms: 250,
                force_reindex: options.force_reindex,
            };

            let scan_engine = Arc::new(ScanEngine::new());
            let metadata_engine = Arc::new(MetadataExtractionEngine::with_performance_profile(
                profile.clone(),
            ));
            let progress_sink: Arc<dyn ProgressSink> =
                Arc::new(IndicatifTerminalProgress::new(false, options.quiet));

            let pipeline = ExtractionPipeline::new(
                scan_engine,
                metadata_engine,
                profile,
                Some(sink.clone()),
                progress_sink,
            );

            let cancel_token = Arc::new(AtomicBool::new(false));
            let cancel_for_signal = cancel_token.clone();
            tokio::spawn(async move {
                if tokio::signal::ctrl_c().await.is_ok() {
                    cancel_for_signal.store(true, std::sync::atomic::Ordering::Relaxed);
                }
            });

            let extract_summary = pipeline
                .execute(source.clone(), None, cancel_token, false)
                .await
                .map_err(|e| {
                    DeduplicationError::ProcessingFailed(format!("Extraction stage failed: {}", e))
                })?;

            stage_durations.push((
                "Extraction".to_string(),
                stage1_start.elapsed().as_secs_f64(),
            ));
            (
                extract_summary.total_discovered as usize,
                extract_summary.total_processed as usize,
            )
        } else {
            if !options.quiet {
                eprintln!(
                    "\n>>> Stage [1/5]: Extraction skipped (organizing all assets cataloged in MongoDB)..."
                );
            }
            stage_durations.push(("Extraction (Skipped)".to_string(), 0.0));
            (0, 0)
        };

        // =========================================================================
        // STAGE 2: Universal Deduplication & Replica Audit
        // =========================================================================
        if !options.quiet {
            eprintln!("\n>>> Stage [2/5]: Auditing Catalog Duplicates & Replicas...");
        }
        let stage2_start = Instant::now();

        let dedup_engine = MediaDeduplicationEngine::new();
        let dedup_summary = dedup_engine
            .run_universal_classification(sink.as_ref(), config, options.dry_run)
            .await?;

        stage_durations.push((
            "Deduplication & Audit".to_string(),
            stage2_start.elapsed().as_secs_f64(),
        ));

        // Setup buffer configuration for transfers
        let buffer_size_bytes =
            TransferBufferConfig::parse_size_str(&options.buffer_size).unwrap_or(8 * 1024 * 1024);
        let buffer_config = TransferBufferConfig::new(
            buffer_size_bytes,
            !options.no_verify_hash,
            options.recheck_disk_hash,
        );

        // =========================================================================
        // STAGE 3: Ingest Unique Files into Canonical Primary Master
        // =========================================================================
        if !options.quiet {
            eprintln!("\n>>> Stage [3/5]: Ingesting Unique Files into Canonical Primary Master...");
        }
        let stage3_start = Instant::now();

        let ingest_service = MasterIngestService::new();
        let source_str = options
            .source_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        let ingest_options = IngestOptions {
            source_drive: source_str.clone(),
            target_drive: None,
            dry_run: options.dry_run,
            delete_source: false, // Deferred to Stage 5 prune
            verify_hash: !options.no_verify_hash,
            buffer_config,
        };

        let ingest_report = ingest_service
            .execute_ingest(sink.as_ref(), config, ingest_options)
            .await?;

        if !ingest_report.errors.is_empty() {
            all_errors.extend(ingest_report.errors.clone());
        }

        stage_durations.push((
            "Master Ingestion".to_string(),
            stage3_start.elapsed().as_secs_f64(),
        ));

        // =========================================================================
        // STAGE 4: Synchronize Replicas to Backup Drives
        // =========================================================================
        if !options.quiet {
            eprintln!("\n>>> Stage [4/5]: Synchronizing Replicas to Backup Drives...");
        }
        let stage4_start = Instant::now();

        let replica_service = ReplicaSyncService::new();
        let replica_options = ReplicaSyncOptions {
            target_replica_id: None,
            primary_drive_id: None,
            dry_run: options.dry_run,
            verify_hash: !options.no_verify_hash,
            buffer_config,
        };

        let replica_report = replica_service
            .execute_sync(sink.as_ref(), config, replica_options)
            .await?;

        if !replica_report.errors.is_empty() {
            all_errors.extend(replica_report.errors.clone());
        }

        stage_durations.push((
            "Replica Synchronization".to_string(),
            stage4_start.elapsed().as_secs_f64(),
        ));

        // =========================================================================
        // STAGE 5: Prune Redundant Files & Clean Empty Directories
        // =========================================================================
        let mut files_pruned = 0usize;
        let mut empty_dirs_removed = 0usize;
        let mut bytes_reclaimed = 0u64;

        if options.skip_prune {
            if !options.quiet {
                eprintln!("\n>>> Stage [5/5]: Prune skipped (--skip-prune specified).");
            }
            stage_durations.push(("Pruning (Skipped)".to_string(), 0.0));
        } else {
            // Safety check: Only proceed with prune if there were zero critical ingest errors
            if !ingest_report.errors.is_empty() && !options.dry_run {
                return Err(DeduplicationError::ProcessingFailed(
                    "Ingestion encountered errors; aborting prune to prevent data loss."
                        .to_string(),
                ));
            }

            if !options.quiet {
                eprintln!(
                    "\n>>> Stage [5/5]: Pruning Redundant Files & Cleaning Empty Directories..."
                );
            }
            let stage5_start = Instant::now();

            let guard = ArchiveProtectionGuard::from_config(config);
            let boundary_ceiling = options.source_path.as_ref().and_then(|sp| {
                DrivePruningService::resolve_drive_boundary_ceiling(sp, config, &guard)
            });

            let pruner_service = DrivePruningService::new();
            let pruner_options = DrivePrunerOptions {
                drive_identifier: options
                    .source_path
                    .as_ref()
                    .map(|p| p.to_string_lossy().to_string()),
                execute: !options.dry_run,
                require_master_disk: true,
                manifest_out: None,
                boundary_ceiling,
                prune_empty_source_root: !options.keep_source_dir,
                clean_empty_dirs: true,
            };

            let prune_report = pruner_service
                .prune_drive(sink.as_ref(), config, pruner_options)
                .await?;

            files_pruned = prune_report.files_deleted;
            empty_dirs_removed = if options.dry_run {
                prune_report.empty_directories_eligible
            } else {
                prune_report.empty_directories_removed
            };
            bytes_reclaimed = prune_report.bytes_reclaimed;

            if !prune_report.errors.is_empty() {
                all_errors.extend(prune_report.errors);
            }

            stage_durations.push(("Pruning".to_string(), stage5_start.elapsed().as_secs_f64()));
        }

        Ok(OrganizePipelineReport {
            source_path: cleaned_source,
            is_dry_run: options.dry_run,
            files_scanned,
            files_extracted,
            canonical_masters_count: dedup_summary.canonical_masters_count,
            replicas_preserved: dedup_summary.replica_files_preserved,
            files_ingested: ingest_report.files_transferred,
            files_replicated: replica_report.files_replicated,
            files_pruned,
            empty_directories_removed: empty_dirs_removed,
            bytes_ingested: ingest_report.bytes_transferred,
            bytes_replicated: replica_report.bytes_transferred,
            bytes_reclaimed,
            total_elapsed_seconds: total_start.elapsed().as_secs_f64(),
            stage_durations,
            errors: all_errors,
        })
    }
}
