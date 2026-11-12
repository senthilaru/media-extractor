use chrono::Utc;
use std::path::{Path, PathBuf};
use std::time::Instant;
use uuid::Uuid;

use crate::application::drive_pruner::DrivePruningService;
use crate::application::transfer_service::UnifiedFileTransferService;
use crate::domain::dedup_models::{
    DeduplicationMetadata, DeduplicationStatus, DriveConfiguration, DriveDefinition,
    IngestExecutionReport, IngestOptions, IngestTask, IngestTaskStatus, MasterIngestPlan,
    MediaFingerprint, PathResolution,
};
use crate::domain::errors::DeduplicationError;
use crate::domain::metadata_models::{FileContext, MediaMetadataDocument};
use crate::domain::models::MediaCategory;
use crate::domain::ports::MetadataStorageSink;
use crate::infrastructure::blake3_hasher::Blake3MediaHasher;
use crate::infrastructure::drives_config::CanonicalPathMatcher;

/// Service orchestrating safe, bitwise-verified ingestion of unmastered media assets
/// into the primary drive's canonical master folder hierarchy.
#[derive(Debug, Default, Clone)]
pub struct MasterIngestService;

impl MasterIngestService {
    pub fn new() -> Self {
        Self
    }

    /// Formats calendar components into `<Year>` and `<DD-Mon-YYYY>` folder name.
    pub fn format_date_folder(year: i32, month: u32, day: u32) -> (i32, String) {
        UnifiedFileTransferService::format_date_folder(year, month, day)
    }

    /// Determines the canonical category folder name.
    pub fn category_folder_name(cat: MediaCategory) -> &'static str {
        UnifiedFileTransferService::category_folder_name(cat)
    }

    /// Resolves non-destructive target path, appending incrementing numeric suffix if needed.
    pub fn resolve_collision_path(target_path: &Path) -> PathBuf {
        let hasher = Blake3MediaHasher::new();
        match UnifiedFileTransferService::resolve_collision_path(target_path, 0, None, &hasher) {
            PathResolution::Available(p)
            | PathResolution::IdenticalExists(p)
            | PathResolution::CollisionResolved(p) => p,
        }
    }

    /// Asynchronously streams file contents from source to destination using a 1MB bounded buffer.
    pub async fn stream_copy_file(source: &Path, target: &Path) -> Result<u64, DeduplicationError> {
        let size = std::fs::metadata(source).map(|m| m.len()).unwrap_or(0);
        UnifiedFileTransferService::stream_copy_verified(source, target, size, None, false).await
    }

    /// Resolves source and target primary drive definitions.
    ///
    /// The source drive can be any external drive mount path, local directory path, or
    /// configured drive ID/path without requiring pre-configuration in `drives.yaml`.
    pub fn resolve_drives<'a>(
        config: &'a DriveConfiguration,
        source_identifier: &str,
        target_identifier: Option<&str>,
    ) -> Result<(DriveDefinition, &'a DriveDefinition), DeduplicationError> {
        let source_drive = DrivePruningService::resolve_drive_or_path(config, source_identifier)?;

        let target_drive = if let Some(target_id) = target_identifier {
            DrivePruningService::resolve_drive(config, target_id).ok_or_else(|| {
                DeduplicationError::ConfigError(format!(
                    "Target primary drive '{}' not found in configuration",
                    target_id
                ))
            })?
        } else {
            // Find highest-priority drive in primary group
            let primary_drives: Vec<&DriveDefinition> = config
                .drives
                .iter()
                .filter(|d| d.group.eq_ignore_ascii_case("primary"))
                .collect();

            if primary_drives.is_empty() {
                return Err(DeduplicationError::ConfigError(
                    "No drive with group 'primary' configured in drives.yaml".to_string(),
                ));
            }
            primary_drives[0]
        };

        Ok((source_drive, target_drive))
    }

    /// Compiles a `MasterIngestPlan` detailing planned file transfers without modifying disk or DB.
    pub async fn compile_plan(
        &self,
        sink: &dyn MetadataStorageSink,
        config: &DriveConfiguration,
        options: &IngestOptions,
    ) -> Result<MasterIngestPlan, DeduplicationError> {
        let (source_drive, target_drive) = Self::resolve_drives(
            config,
            &options.source_drive,
            options.target_drive.as_deref(),
        )?;

        // Verify source drive mount
        if !source_drive.mount_path.exists() {
            return Err(DeduplicationError::DriveNotMounted {
                drive: source_drive.id.clone(),
                path: source_drive.mount_path.clone(),
            });
        }

        // Verify target drive mount
        if !target_drive.mount_path.exists() {
            return Err(DeduplicationError::DriveNotMounted {
                drive: target_drive.id.clone(),
                path: target_drive.mount_path.clone(),
            });
        }

        let source_mount_str = source_drive.mount_path.to_string_lossy();
        let unmastered_docs = sink
            .query_unmastered_candidates(Some(&source_mount_str))
            .await?;

        let mut tasks = Vec::with_capacity(unmastered_docs.len());
        let mut total_bytes = 0u64;
        let hasher = Blake3MediaHasher::new();

        for doc in unmastered_docs {
            let cat = doc.file_context.media_category;

            // 1. Resolve date components
            let (year, month, day) = if doc.temporal.year > 0 {
                (doc.temporal.year, doc.temporal.month, doc.temporal.day)
            } else if let Some(mtime) = doc.file_context.modified_at {
                use chrono::Datelike;
                (mtime.year(), mtime.month(), mtime.day())
            } else {
                (2026, 1, 1)
            };

            let rel_path = UnifiedFileTransferService::build_canonical_relative_path(
                year,
                month,
                day,
                cat,
                &doc.file_context.file_name,
            );

            let initial_target_path = target_drive.master_path.join(&rel_path);

            // 2. Collision resolution
            let source_hash = doc
                .fingerprint
                .as_ref()
                .map(|f| f.cryptographic_hash.clone());

            let resolution = UnifiedFileTransferService::resolve_collision_path(
                &initial_target_path,
                doc.file_context.file_size_bytes,
                source_hash.as_deref(),
                &hasher,
            );

            let (target_path, status) = match resolution {
                PathResolution::Available(p) => (p, IngestTaskStatus::Pending),
                PathResolution::IdenticalExists(p) => (p, IngestTaskStatus::SkippedIdentical),
                PathResolution::CollisionResolved(p) => (p, IngestTaskStatus::Pending),
            };

            if status == IngestTaskStatus::Pending {
                total_bytes += doc.file_context.file_size_bytes;
            }

            tasks.push(IngestTask {
                source_doc_id: doc.id,
                source_path: doc.file_context.file_path,
                target_path,
                category: cat,
                file_size_bytes: doc.file_context.file_size_bytes,
                cryptographic_hash: source_hash,
                status,
            });
        }

        Ok(MasterIngestPlan {
            source_drive_id: source_drive.id.clone(),
            target_drive_id: target_drive.id.clone(),
            source_mount: source_drive.mount_path.clone(),
            target_master_root: target_drive.master_path.clone(),
            tasks,
            total_bytes,
            available_target_bytes: u64::MAX,
        })
    }

    /// Executes the full ingestion process according to options.
    pub async fn execute_ingest(
        &self,
        sink: &dyn MetadataStorageSink,
        config: &DriveConfiguration,
        options: IngestOptions,
    ) -> Result<IngestExecutionReport, DeduplicationError> {
        let start = Instant::now();
        let plan = self.compile_plan(sink, config, &options).await?;

        if options.dry_run {
            return Ok(IngestExecutionReport {
                source_drive_id: plan.source_drive_id,
                target_drive_id: plan.target_drive_id,
                is_dry_run: true,
                total_files_planned: plan.tasks.len(),
                files_transferred: 0,
                files_skipped_identical: plan
                    .tasks
                    .iter()
                    .filter(|t| t.status == IngestTaskStatus::SkippedIdentical)
                    .count(),
                files_failed: 0,
                bytes_transferred: 0,
                source_files_deleted: 0,
                elapsed_seconds: start.elapsed().as_secs_f64(),
                errors: Vec::new(),
            });
        }

        let matcher = CanonicalPathMatcher::new(config.clone());
        let mut files_transferred = 0usize;
        let mut files_skipped_identical = 0usize;
        let mut files_failed = 0usize;
        let mut bytes_transferred = 0u64;
        let mut source_files_deleted = 0usize;
        let mut errors = Vec::new();

        let mut source_updates: Vec<(Uuid, DeduplicationMetadata)> = Vec::new();
        let mut new_master_docs: Vec<MediaMetadataDocument> = Vec::new();
        let mut master_links: Vec<(String, Uuid, PathBuf)> = Vec::new();

        for task in &plan.tasks {
            match task.status {
                IngestTaskStatus::SkippedIdentical => {
                    files_skipped_identical += 1;
                    let new_master_id = Uuid::new_v4();
                    let now = Utc::now();
                    let master_meta = DeduplicationMetadata {
                        cluster_id: Uuid::new_v4(),
                        status: DeduplicationStatus::CanonicalMaster,
                        canonical_master_id: Some(new_master_id),
                        canonical_master_path: Some(task.target_path.clone()),
                        drive_id: Some(plan.target_drive_id.clone()),
                        drive_group: Some("primary".to_string()),
                        is_in_master_path: true,
                        reclaimable_bytes: 0,
                        evaluated_at: now,
                    };

                    let original_doc = sink
                        .find_document_by_id(task.source_doc_id)
                        .await
                        .ok()
                        .flatten();

                    let effective_hash = task.cryptographic_hash.clone().or_else(|| {
                        original_doc
                            .as_ref()
                            .and_then(|d| d.fingerprint.as_ref())
                            .map(|f| f.cryptographic_hash.clone())
                    });

                    let mut new_doc = match original_doc {
                        Some(mut d) => {
                            d.id = new_master_id;
                            d.file_context.scanned_root = plan.target_master_root.clone();
                            d.file_context.file_path = task.target_path.clone();
                            d.file_context.file_name = task
                                .target_path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            d.dedup = Some(master_meta);
                            d
                        }
                        None => MediaMetadataDocument {
                            id: new_master_id,
                            file_context: FileContext {
                                scanned_root: plan.target_master_root.clone(),
                                file_path: task.target_path.clone(),
                                relative_path: task.target_path.clone(),
                                file_name: task
                                    .target_path
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string(),
                                file_size_bytes: task.file_size_bytes,
                                extension: task
                                    .target_path
                                    .extension()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .to_string(),
                                media_category: task.category,
                                modified_at: Some(now),
                            },
                            temporal: crate::domain::metadata_models::TemporalMetadata::from_utc_datetime(
                                now,
                                crate::domain::metadata_models::TimestampSource::ExifDateTimeOriginal,
                            ),
                            camera: None,
                            exposure: None,
                            location: None,
                            dimensions: None,
                            video: None,
                            fingerprint: None,
                            extraction_status: Default::default(),
                            dedup: Some(master_meta),
                        },
                    };

                    if new_doc.fingerprint.is_none() {
                        if let Some(ref hash) = effective_hash {
                            new_doc.fingerprint = Some(MediaFingerprint {
                                id: Uuid::new_v4(),
                                file_path: task.target_path.clone(),
                                file_size_bytes: task.file_size_bytes,
                                media_category: task.category,
                                dimensions: None,
                                sample_digest: None,
                                cryptographic_hash: hash.clone(),
                                perceptual_hash: None,
                                phash_hex: None,
                                rotated_hashes: None,
                                is_low_entropy: false,
                                fingerprinted_at: now,
                            });
                        }
                    }

                    new_master_docs.push(new_doc);

                    if let Some(ref hash) = effective_hash {
                        master_links.push((hash.clone(), new_master_id, task.target_path.clone()));
                    }

                    let meta = DeduplicationMetadata {
                        cluster_id: Uuid::new_v4(),
                        status: DeduplicationStatus::MarkForRemoval,
                        canonical_master_id: Some(new_master_id),
                        canonical_master_path: Some(task.target_path.clone()),
                        drive_id: Some(plan.source_drive_id.clone()),
                        drive_group: None,
                        is_in_master_path: false,
                        reclaimable_bytes: task.file_size_bytes,
                        evaluated_at: now,
                    };
                    source_updates.push((task.source_doc_id, meta));
                }
                IngestTaskStatus::Pending => {
                    // Use shared UnifiedFileTransferService for verified streaming copy
                    match UnifiedFileTransferService::stream_copy_verified(
                        &task.source_path,
                        &task.target_path,
                        task.file_size_bytes,
                        task.cryptographic_hash.as_deref(),
                        options.verify_hash,
                    )
                    .await
                    {
                        Ok(copied_bytes) => {
                            files_transferred += 1;
                            bytes_transferred += copied_bytes;

                            // Create canonical master document on primary
                            let new_master_id = Uuid::new_v4();
                            let now = Utc::now();
                            let master_meta = DeduplicationMetadata {
                                cluster_id: Uuid::new_v4(),
                                status: DeduplicationStatus::CanonicalMaster,
                                canonical_master_id: Some(new_master_id),
                                canonical_master_path: Some(task.target_path.clone()),
                                drive_id: Some(plan.target_drive_id.clone()),
                                drive_group: Some("primary".to_string()),
                                is_in_master_path: true,
                                reclaimable_bytes: 0,
                                evaluated_at: now,
                            };

                            let original_doc = sink
                                .find_document_by_id(task.source_doc_id)
                                .await
                                .ok()
                                .flatten();

                            let effective_hash = task.cryptographic_hash.clone().or_else(|| {
                                original_doc
                                    .as_ref()
                                    .and_then(|d| d.fingerprint.as_ref())
                                    .map(|f| f.cryptographic_hash.clone())
                            });

                            let mut new_doc = match original_doc {
                                Some(mut d) => {
                                    d.id = new_master_id;
                                    d.file_context.scanned_root = plan.target_master_root.clone();
                                    d.file_context.file_path = task.target_path.clone();
                                    d.file_context.file_name = task
                                        .target_path
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .to_string();
                                    d.dedup = Some(master_meta);
                                    d
                                }
                                None => MediaMetadataDocument {
                                    id: new_master_id,
                                    file_context: FileContext {
                                        scanned_root: plan.target_master_root.clone(),
                                        file_path: task.target_path.clone(),
                                        relative_path: task.target_path.clone(),
                                        file_name: task
                                            .target_path
                                            .file_name()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .to_string(),
                                        file_size_bytes: task.file_size_bytes,
                                        extension: task
                                            .target_path
                                            .extension()
                                            .unwrap_or_default()
                                            .to_string_lossy()
                                            .to_string(),
                                        media_category: task.category,
                                        modified_at: Some(now),
                                    },
                                    temporal: crate::domain::metadata_models::TemporalMetadata::from_utc_datetime(
                                        now,
                                        crate::domain::metadata_models::TimestampSource::ExifDateTimeOriginal,
                                    ),
                                    camera: None,
                                    exposure: None,
                                    location: None,
                                    dimensions: None,
                                    video: None,
                                    fingerprint: None,
                                    extraction_status: Default::default(),
                                    dedup: Some(master_meta),
                                },
                            };

                            if new_doc.fingerprint.is_none() {
                                if let Some(ref hash) = effective_hash {
                                    new_doc.fingerprint = Some(MediaFingerprint {
                                        id: Uuid::new_v4(),
                                        file_path: task.target_path.clone(),
                                        file_size_bytes: task.file_size_bytes,
                                        media_category: task.category,
                                        dimensions: None,
                                        sample_digest: None,
                                        cryptographic_hash: hash.clone(),
                                        perceptual_hash: None,
                                        phash_hex: None,
                                        rotated_hashes: None,
                                        is_low_entropy: false,
                                        fingerprinted_at: now,
                                    });
                                }
                            }

                            new_master_docs.push(new_doc);

                            if let Some(ref hash) = effective_hash {
                                master_links.push((
                                    hash.clone(),
                                    new_master_id,
                                    task.target_path.clone(),
                                ));
                            }

                            // Check if source file is on a replica drive in canonical master structure
                            let is_source_on_replica_master = matcher
                                .is_in_canonical_master(&task.source_path)
                                && matcher
                                    .find_drive_for_path(&task.source_path)
                                    .map(|d| d.group.starts_with("replica"))
                                    .unwrap_or(false);

                            let mut source_status = if is_source_on_replica_master {
                                DeduplicationStatus::ReplicaKeep
                            } else {
                                DeduplicationStatus::MarkForRemoval
                            };

                            if options.delete_source
                                && task.source_path.exists()
                                && !is_source_on_replica_master
                            {
                                match std::fs::remove_file(&task.source_path) {
                                    Ok(_) => {
                                        source_files_deleted += 1;
                                        source_status = DeduplicationStatus::Removed;
                                    }
                                    Err(e) => {
                                        errors.push(format!(
                                            "Failed deleting source file '{}': {}",
                                            task.source_path.display(),
                                            e
                                        ));
                                    }
                                }
                            }

                            let source_meta = DeduplicationMetadata {
                                cluster_id: Uuid::new_v4(),
                                status: source_status,
                                canonical_master_id: Some(new_master_id),
                                canonical_master_path: Some(task.target_path.clone()),
                                drive_id: Some(plan.source_drive_id.clone()),
                                drive_group: None,
                                is_in_master_path: is_source_on_replica_master,
                                reclaimable_bytes: if is_source_on_replica_master {
                                    0
                                } else {
                                    task.file_size_bytes
                                },
                                evaluated_at: now,
                            };
                            source_updates.push((task.source_doc_id, source_meta));
                        }
                        Err(e) => {
                            files_failed += 1;
                            errors.push(format!("{}", e));
                        }
                    }
                }
                IngestTaskStatus::CopiedVerified => {}
                IngestTaskStatus::Failed(ref err) => {
                    files_failed += 1;
                    errors.push(err.clone());
                }
            }
        }

        // Commit database writes
        if !new_master_docs.is_empty() {
            if let Err(e) = sink.save_batch(&new_master_docs).await {
                errors.push(format!(
                    "Failed saving new master records to database: {}",
                    e
                ));
            }
        }

        if !source_updates.is_empty() {
            if let Err(e) = sink.update_dedup_batch(&source_updates).await {
                errors.push(format!("Failed updating source records in database: {}", e));
            }
        }

        if !master_links.is_empty() {
            if let Err(e) = sink.link_canonical_masters(&master_links).await {
                errors.push(format!(
                    "Failed linking duplicate records to canonical master: {}",
                    e
                ));
            }
        }

        Ok(IngestExecutionReport {
            source_drive_id: plan.source_drive_id,
            target_drive_id: plan.target_drive_id,
            is_dry_run: false,
            total_files_planned: plan.tasks.len(),
            files_transferred,
            files_skipped_identical,
            files_failed,
            bytes_transferred,
            source_files_deleted,
            elapsed_seconds: start.elapsed().as_secs_f64(),
            errors,
        })
    }
}
