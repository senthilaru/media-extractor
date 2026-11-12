use chrono::Utc;
use std::collections::HashSet;
use std::time::Instant;
use uuid::Uuid;

use crate::application::drive_pruner::DrivePruningService;
use crate::application::transfer_service::UnifiedFileTransferService;
use crate::domain::dedup_models::{
    DeduplicationMetadata, DeduplicationStatus, DriveConfiguration, DriveDefinition,
    IngestTaskStatus, PathResolution, ReplicaSyncPlan, ReplicaSyncReport, ReplicaSyncTask,
};
use crate::domain::errors::DeduplicationError;
use crate::domain::metadata_models::{FileContext, MediaMetadataDocument};
use crate::domain::ports::MetadataStorageSink;
use crate::infrastructure::blake3_hasher::Blake3MediaHasher;

/// Runtime options for a replica synchronization run.
#[derive(Debug, Clone)]
pub struct ReplicaSyncOptions {
    pub target_replica_id: Option<String>,
    pub primary_drive_id: Option<String>,
    pub dry_run: bool,
    pub verify_hash: bool,
}

/// Service orchestrating fanout replication of CanonicalMaster media assets
/// to all configured and mounted replica drives.
#[derive(Debug, Default, Clone)]
pub struct ReplicaSyncService;

impl ReplicaSyncService {
    pub fn new() -> Self {
        Self
    }

    /// Resolves primary drive and candidate replica drives from configuration.
    pub fn resolve_drives<'a>(
        config: &'a DriveConfiguration,
        primary_id: Option<&str>,
        target_replica_id: Option<&str>,
    ) -> Result<(&'a DriveDefinition, Vec<&'a DriveDefinition>), DeduplicationError> {
        let primary_drive = if let Some(p_id) = primary_id {
            DrivePruningService::resolve_drive(config, p_id).ok_or_else(|| {
                DeduplicationError::ConfigError(format!(
                    "Primary drive '{}' not found in configuration",
                    p_id
                ))
            })?
        } else {
            let primaries: Vec<&DriveDefinition> = config
                .drives
                .iter()
                .filter(|d| d.group.eq_ignore_ascii_case("primary"))
                .collect();
            if primaries.is_empty() {
                return Err(DeduplicationError::ConfigError(
                    "No drive with group 'primary' configured in drives.yaml".to_string(),
                ));
            }
            primaries[0]
        };

        let replica_drives: Vec<&DriveDefinition> = if let Some(target_id) = target_replica_id {
            let rep = DrivePruningService::resolve_drive(config, target_id).ok_or_else(|| {
                DeduplicationError::ConfigError(format!(
                    "Target replica drive '{}' not found in configuration",
                    target_id
                ))
            })?;
            vec![rep]
        } else {
            config
                .drives
                .iter()
                .filter(|d| {
                    d.id != primary_drive.id
                        && (d.group.starts_with("replica")
                            || d.group.eq_ignore_ascii_case("replica"))
                })
                .collect()
        };

        Ok((primary_drive, replica_drives))
    }

    /// Compiles a ReplicaSyncPlan detailing all missing files on mounted replica drives.
    pub async fn compile_plan(
        &self,
        sink: &dyn MetadataStorageSink,
        config: &DriveConfiguration,
        options: &ReplicaSyncOptions,
    ) -> Result<ReplicaSyncPlan, DeduplicationError> {
        let (primary_drive, replica_drives) = Self::resolve_drives(
            config,
            options.primary_drive_id.as_deref(),
            options.target_replica_id.as_deref(),
        )?;

        // Verify primary drive mount
        if !primary_drive.mount_path.exists() {
            return Err(DeduplicationError::DriveNotMounted {
                drive: primary_drive.id.clone(),
                path: primary_drive.mount_path.clone(),
            });
        }

        let mut active_replica_ids = Vec::new();
        let mut offline_replica_ids = Vec::new();
        let mut active_replica_drives = Vec::new();

        for &rep in &replica_drives {
            if rep.mount_path.exists() {
                active_replica_ids.push(rep.id.clone());
                active_replica_drives.push(rep);
            } else {
                offline_replica_ids.push(rep.id.clone());
            }
        }

        let canonical_masters = sink.query_canonical_masters().await?;
        let mut tasks = Vec::new();
        let mut total_bytes = 0u64;
        let hasher = Blake3MediaHasher::new();

        for rep in active_replica_drives {
            // Query catalog records already present on this replica drive
            let existing_replicas = sink.query_replica_documents(&rep.id).await?;
            let existing_hashes: HashSet<String> = existing_replicas
                .iter()
                .filter_map(|d| d.fingerprint.as_ref().map(|f| f.cryptographic_hash.clone()))
                .collect();
            let existing_master_ids: HashSet<Uuid> = existing_replicas
                .iter()
                .filter_map(|d| d.dedup.as_ref().and_then(|meta| meta.canonical_master_id))
                .collect();

            for master in &canonical_masters {
                let master_hash = master
                    .fingerprint
                    .as_ref()
                    .map(|f| f.cryptographic_hash.clone());

                let is_already_registered = existing_master_ids.contains(&master.id)
                    || master_hash
                        .as_ref()
                        .map(|h| existing_hashes.contains(h))
                        .unwrap_or(false);

                // Derive canonical relative path
                let rel_path = match master
                    .file_context
                    .file_path
                    .strip_prefix(&primary_drive.master_path)
                {
                    Ok(rel) => rel.to_path_buf(),
                    Err(_) => {
                        let (year, month, day) = if master.temporal.year > 0 {
                            (
                                master.temporal.year,
                                master.temporal.month,
                                master.temporal.day,
                            )
                        } else if let Some(mtime) = master.file_context.modified_at {
                            use chrono::Datelike;
                            (mtime.year(), mtime.month(), mtime.day())
                        } else {
                            (2026, 1, 1)
                        };
                        UnifiedFileTransferService::build_canonical_relative_path(
                            year,
                            month,
                            day,
                            master.file_context.media_category,
                            &master.file_context.file_name,
                        )
                    }
                };

                let target_path = rep.master_path.join(&rel_path);

                let resolution = UnifiedFileTransferService::resolve_collision_path(
                    &target_path,
                    master.file_context.file_size_bytes,
                    master_hash.as_deref(),
                    &hasher,
                );

                let (dest_path, status) = match resolution {
                    PathResolution::IdenticalExists(p) => {
                        if is_already_registered {
                            continue; // Already on disk and registered in DB
                        }
                        (p, IngestTaskStatus::SkippedIdentical)
                    }
                    PathResolution::Available(p) | PathResolution::CollisionResolved(p) => {
                        (p, IngestTaskStatus::Pending)
                    }
                };

                if status == IngestTaskStatus::Pending {
                    total_bytes += master.file_context.file_size_bytes;
                }

                tasks.push(ReplicaSyncTask {
                    task_id: Uuid::new_v4(),
                    canonical_master_id: master.id,
                    source_path: master.file_context.file_path.clone(),
                    target_path: dest_path,
                    replica_drive_id: rep.id.clone(),
                    replica_group: rep.group.clone(),
                    canonical_rel_path: rel_path,
                    file_size_bytes: master.file_context.file_size_bytes,
                    cryptographic_hash: master_hash,
                    status,
                });
            }
        }

        Ok(ReplicaSyncPlan {
            primary_drive_id: primary_drive.id.clone(),
            primary_master_root: primary_drive.master_path.clone(),
            active_replica_ids,
            offline_replica_ids,
            tasks,
            total_bytes,
        })
    }

    /// Executes the replication plan, copying missing files to replicas, verifying hashes,
    /// inserting ReplicaKeep documents into the database, and returning a detailed report.
    pub async fn execute_sync(
        &self,
        sink: &dyn MetadataStorageSink,
        config: &DriveConfiguration,
        options: ReplicaSyncOptions,
    ) -> Result<ReplicaSyncReport, DeduplicationError> {
        let start = Instant::now();
        let plan = self.compile_plan(sink, config, &options).await?;

        if options.dry_run {
            return Ok(ReplicaSyncReport {
                primary_drive_id: plan.primary_drive_id,
                is_dry_run: true,
                synchronized_replicas: plan.active_replica_ids,
                offline_replicas: plan.offline_replica_ids,
                total_files_planned: plan.tasks.len(),
                files_replicated: 0,
                files_skipped_identical: plan
                    .tasks
                    .iter()
                    .filter(|t| t.status == IngestTaskStatus::SkippedIdentical)
                    .count(),
                files_failed: 0,
                bytes_transferred: 0,
                elapsed_seconds: start.elapsed().as_secs_f64(),
                errors: Vec::new(),
            });
        }

        let mut files_replicated = 0usize;
        let mut files_skipped_identical = 0usize;
        let mut files_failed = 0usize;
        let mut bytes_transferred = 0u64;
        let mut errors = Vec::new();
        let mut new_replica_docs = Vec::new();

        for task in &plan.tasks {
            match task.status {
                IngestTaskStatus::SkippedIdentical => {
                    files_skipped_identical += 1;
                    let now = Utc::now();
                    let new_doc_id = Uuid::new_v4();
                    let rep_meta = DeduplicationMetadata {
                        cluster_id: Uuid::new_v4(),
                        status: DeduplicationStatus::ReplicaKeep,
                        canonical_master_id: Some(task.canonical_master_id),
                        canonical_master_path: Some(task.source_path.clone()),
                        drive_id: Some(task.replica_drive_id.clone()),
                        drive_group: Some(task.replica_group.clone()),
                        is_in_master_path: true,
                        reclaimable_bytes: 0,
                        evaluated_at: now,
                    };

                    let master_doc = sink
                        .find_document_by_id(task.canonical_master_id)
                        .await
                        .ok()
                        .flatten();

                    let replica_doc = match master_doc {
                        Some(mut d) => {
                            d.id = new_doc_id;
                            d.file_context.file_path = task.target_path.clone();
                            d.file_context.file_name = task
                                .target_path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            d.dedup = Some(rep_meta);
                            d
                        }
                        None => MediaMetadataDocument {
                            id: new_doc_id,
                            file_context: FileContext {
                                scanned_root: task.target_path.clone(),
                                file_path: task.target_path.clone(),
                                relative_path: task.canonical_rel_path.clone(),
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
                                media_category: crate::domain::models::MediaCategory::Photo,
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
                            dedup: Some(rep_meta),
                        },
                    };
                    new_replica_docs.push(replica_doc);
                }
                IngestTaskStatus::Pending => {
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
                            files_replicated += 1;
                            bytes_transferred += copied_bytes;

                            let now = Utc::now();
                            let new_doc_id = Uuid::new_v4();
                            let rep_meta = DeduplicationMetadata {
                                cluster_id: Uuid::new_v4(),
                                status: DeduplicationStatus::ReplicaKeep,
                                canonical_master_id: Some(task.canonical_master_id),
                                canonical_master_path: Some(task.source_path.clone()),
                                drive_id: Some(task.replica_drive_id.clone()),
                                drive_group: Some(task.replica_group.clone()),
                                is_in_master_path: true,
                                reclaimable_bytes: 0,
                                evaluated_at: now,
                            };

                            let master_doc = sink
                                .find_document_by_id(task.canonical_master_id)
                                .await
                                .ok()
                                .flatten();

                            let replica_doc = match master_doc {
                                Some(mut d) => {
                                    d.id = new_doc_id;
                                    d.file_context.file_path = task.target_path.clone();
                                    d.file_context.file_name = task
                                        .target_path
                                        .file_name()
                                        .unwrap_or_default()
                                        .to_string_lossy()
                                        .to_string();
                                    d.dedup = Some(rep_meta);
                                    d
                                }
                                None => MediaMetadataDocument {
                                    id: new_doc_id,
                                    file_context: FileContext {
                                        scanned_root: task.target_path.clone(),
                                        file_path: task.target_path.clone(),
                                        relative_path: task.canonical_rel_path.clone(),
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
                                        media_category: crate::domain::models::MediaCategory::Photo,
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
                                    dedup: Some(rep_meta),
                                },
                            };
                            new_replica_docs.push(replica_doc);
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

        if !new_replica_docs.is_empty() {
            if let Err(e) = sink.save_batch(&new_replica_docs).await {
                errors.push(format!("Failed saving replica records to database: {}", e));
            }
        }

        Ok(ReplicaSyncReport {
            primary_drive_id: plan.primary_drive_id,
            is_dry_run: false,
            synchronized_replicas: plan.active_replica_ids,
            offline_replicas: plan.offline_replica_ids,
            total_files_planned: plan.tasks.len(),
            files_replicated,
            files_skipped_identical,
            files_failed,
            bytes_transferred,
            elapsed_seconds: start.elapsed().as_secs_f64(),
            errors,
        })
    }
}
