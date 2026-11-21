use chrono::Utc;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::time::Instant;
use uuid::Uuid;

use crate::application::drive_pruner::DrivePruningService;
use crate::application::transfer_service::UnifiedFileTransferService;
use crate::domain::dedup_models::{
    DeduplicationMetadata, DeduplicationStatus, DriveConfiguration, DriveDefinition,
    IngestExecutionReport, IngestOptions, IngestTask, IngestTaskStatus, MasterIngestPlan,
    MasterReplicationState, MediaFingerprint, PathResolution, ReplicationStatus,
    TransferBufferConfig,
};
use crate::domain::errors::DeduplicationError;
use crate::domain::metadata_models::{FileContext, MediaMetadataDocument};
use crate::domain::models::MediaCategory;
use crate::domain::ports::{MediaClassifier, MetadataStorageSink};
use crate::infrastructure::archive_guard::ArchiveProtectionGuard;
use crate::infrastructure::blake3_hasher::Blake3MediaHasher;
use crate::infrastructure::drive_spindle_lock::DriveSpindleLockManager;
use std::sync::Arc;
use crate::infrastructure::disk_probe::{DiskSpaceProbe, PosixDiskSpaceProbe};
use crate::infrastructure::drives_config::CanonicalPathMatcher;

/// Default minimum free-space safety buffer on storage pool volumes (5 GB).
pub const DEFAULT_MIN_FREE_SPACE_BUFFER_BYTES: u64 = 5 * 1024 * 1024 * 1024;

/// Service orchestrating safe, bitwise-verified ingestion of unmastered media assets
/// into the primary drive's canonical master folder hierarchy.
#[derive(Debug, Clone)]
pub struct MasterIngestService {
    spindle_locks: DriveSpindleLockManager,
    disk_probe: Arc<dyn DiskSpaceProbe>,
    min_free_space_buffer: u64,
}

impl Default for MasterIngestService {
    fn default() -> Self {
        Self {
            spindle_locks: DriveSpindleLockManager::default(),
            disk_probe: Arc::new(PosixDiskSpaceProbe::new()),
            min_free_space_buffer: DEFAULT_MIN_FREE_SPACE_BUFFER_BYTES,
        }
    }
}

impl MasterIngestService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_spindle_locks(spindle_locks: DriveSpindleLockManager) -> Self {
        Self {
            spindle_locks,
            ..Default::default()
        }
    }

    pub fn with_probe(mut self, probe: Arc<dyn DiskSpaceProbe>) -> Self {
        self.disk_probe = probe;
        self
    }

    pub fn with_min_free_space_buffer(mut self, buffer_bytes: u64) -> Self {
        self.min_free_space_buffer = buffer_bytes;
        self
    }

    /// Resolves the optimal target volume within a storage pool for a file of `file_size_bytes`.
    /// Evaluates volumes in pool priority sequence; spills over to the next volume if available
    /// disk space is less than `file_size_bytes + min_free_space_buffer`.
    pub fn resolve_pool_target_volume<'a>(
        &self,
        pool_volumes: &[&'a DriveDefinition],
        file_size_bytes: u64,
        probe: &dyn DiskSpaceProbe,
        min_free_space_buffer: u64,
    ) -> Result<&'a DriveDefinition, DeduplicationError> {
        let mut max_avail = 0u64;
        let pool_name = pool_volumes
            .first()
            .map(|v| v.group.clone())
            .unwrap_or_else(|| "primary".to_string());

        for vol in pool_volumes {
            let avail = probe.available_space_bytes(&vol.mount_path).unwrap_or(0);
            if avail > max_avail {
                max_avail = avail;
            }
            if avail >= file_size_bytes.saturating_add(min_free_space_buffer) {
                return Ok(vol);
            }
        }

        Err(DeduplicationError::StoragePoolExhausted {
            pool: pool_name,
            required_bytes: file_size_bytes.saturating_add(min_free_space_buffer),
            available_bytes: max_avail,
        })
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

    /// Asynchronously streams file contents from source to destination using a tuned bounded buffer.
    pub async fn stream_copy_file(source: &Path, target: &Path) -> Result<u64, DeduplicationError> {
        let size = std::fs::metadata(source).map(|m| m.len()).unwrap_or(0);
        UnifiedFileTransferService::stream_copy_verified(
            source,
            target,
            size,
            None,
            TransferBufferConfig::default(),
        )
        .await
    }

    /// Resolves source and target master drive definitions.
    ///
    /// The source drive can be any external drive mount path, local directory path, or
    /// configured drive ID/path without requiring pre-configuration in `drives.yaml`.
    ///
    /// When `target_identifier` is omitted, the highest priority drive currently mounted on disk
    /// is chosen (supporting either primary or secondary drives without requiring both to be connected).
    pub fn resolve_drives<'a>(
        config: &'a DriveConfiguration,
        source_identifier: &str,
        target_identifier: Option<&str>,
    ) -> Result<(DriveDefinition, &'a DriveDefinition), DeduplicationError> {
        let source_drive = DrivePruningService::resolve_drive_or_path(config, source_identifier)?;

        let target_drive = if let Some(target_id) = target_identifier {
            DrivePruningService::resolve_drive(config, target_id)
                .or_else(|| {
                    config
                        .drives
                        .iter()
                        .find(|d| d.group.eq_ignore_ascii_case(target_id))
                })
                .ok_or_else(|| {
                    DeduplicationError::ConfigError(format!(
                        "Target master drive '{}' not found in configuration",
                        target_id
                    ))
                })?
        } else {
            if config.drives.is_empty() {
                return Err(DeduplicationError::ConfigError(
                    "No drives configured in drives.yaml".to_string(),
                ));
            }

            // Rank configured drives by group priority descending
            let mut candidates: Vec<&DriveDefinition> = config.drives.iter().collect();
            candidates.sort_by(|a, b| {
                let prio_a = config.groups.get(&a.group).map(|g| g.priority).unwrap_or(0);
                let prio_b = config.groups.get(&b.group).map(|g| g.priority).unwrap_or(0);
                prio_b.cmp(&prio_a).then_with(|| a.id.cmp(&b.id))
            });

            // Find first drive that is currently mounted on disk
            let mounted = candidates.iter().copied().find(|d| d.mount_path.exists());

            match mounted {
                Some(drive) => drive,
                None => {
                    // Fall back to highest priority candidate so caller can emit DriveNotMounted
                    candidates[0]
                }
            }
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
        let is_all_sources = options.source_drive.trim().is_empty()
            || options.source_drive.eq_ignore_ascii_case("all")
            || options.source_drive.eq_ignore_ascii_case("database");

        let (source_drive_id, source_mount_path, source_mount_str, target_drive) = if is_all_sources {
            let target_drive = if let Some(target_id) = options.target_drive.as_deref() {
                DrivePruningService::resolve_drive(config, target_id)
                    .or_else(|| {
                        config
                            .drives
                            .iter()
                            .find(|d| d.group.eq_ignore_ascii_case(target_id))
                    })
                    .ok_or_else(|| {
                        DeduplicationError::ConfigError(format!(
                            "Target master drive '{}' not found in configuration",
                            target_id
                        ))
                    })?
            } else {
                if config.drives.is_empty() {
                    return Err(DeduplicationError::ConfigError(
                        "No drives configured in drives.yaml".to_string(),
                    ));
                }

                let mut candidates: Vec<&DriveDefinition> = config.drives.iter().collect();
                candidates.sort_by(|a, b| {
                    let prio_a = config.groups.get(&a.group).map(|g| g.priority).unwrap_or(0);
                    let prio_b = config.groups.get(&b.group).map(|g| g.priority).unwrap_or(0);
                    prio_b.cmp(&prio_a).then_with(|| a.id.cmp(&b.id))
                });

                let mounted = candidates.iter().copied().find(|d| d.mount_path.exists());
                match mounted {
                    Some(drive) => drive,
                    None => candidates[0],
                }
            };

            // Verify target drive mount
            if !target_drive.mount_path.exists() {
                return Err(DeduplicationError::DriveNotMounted {
                    drive: target_drive.id.clone(),
                    path: target_drive.mount_path.clone(),
                });
            }

            (
                "database".to_string(),
                PathBuf::from("(Catalog Database Candidates)"),
                None,
                target_drive,
            )
        } else {
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

            let mount_str = source_drive.mount_path.to_string_lossy().to_string();
            (
                source_drive.id.clone(),
                source_drive.mount_path.clone(),
                Some(mount_str),
                target_drive,
            )
        };

        let unmastered_docs = sink
            .query_unmastered_candidates(source_mount_str.as_deref())
            .await?;

        let canonical_masters = if unmastered_docs
            .iter()
            .any(|d| d.file_context.media_category == MediaCategory::Sidecar)
        {
            sink.query_canonical_masters().await.unwrap_or_default()
        } else {
            Vec::new()
        };

        let matcher = CanonicalPathMatcher::new(config.clone());
        let target_pool_volumes = config.get_pool_volumes(&target_drive.group);
        let pool_volumes: Vec<&DriveDefinition> = if target_pool_volumes.is_empty() {
            vec![target_drive]
        } else {
            target_pool_volumes
        };

        let mut volume_avail_space: std::collections::HashMap<String, u64> =
            std::collections::HashMap::new();
        for vol in &pool_volumes {
            let avail = self
                .disk_probe
                .available_space_bytes(&vol.mount_path)
                .unwrap_or(u64::MAX);
            volume_avail_space.insert(vol.id.clone(), avail);
        }

        let mut tasks: Vec<IngestTask> = Vec::with_capacity(unmastered_docs.len());
        let mut total_bytes = 0u64;
        let hasher = Blake3MediaHasher::new();

        for doc in unmastered_docs {
            if is_all_sources && !doc.file_context.file_path.exists() {
                tracing::warn!(
                    path = %doc.file_context.file_path.display(),
                    "Skipping unmastered candidate: source file does not exist on disk (unmounted drive?)"
                );
                continue;
            }
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

            // If this item is a Sidecar (.aae, .ifo, .bup), check if its companion is in current batch, existing masters, or on disk
            let mut resolved_rel_path = None;
            if cat == MediaCategory::Sidecar {
                let stem = Path::new(&doc.file_context.file_name)
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default();
                let is_dvd = doc.file_context.extension.eq_ignore_ascii_case("ifo")
                    || doc.file_context.extension.eq_ignore_ascii_case("bup");

                // Check 1: Companion in current plan tasks (being ingested in same batch)
                for task in &tasks {
                    if let Some(task_stem) = task.source_path.file_stem().and_then(|s| s.to_str()) {
                        if UnifiedFileTransferService::is_companion_sidecar_stem_match(task_stem, stem, &doc.file_context.extension) {
                            if let Some(task_parent) = task.target_path.parent() {
                                let rel_parent = if let Ok(rel) = task_parent.strip_prefix(&target_drive.master_path) {
                                    Some(rel.to_path_buf())
                                } else if let Some(drive) = matcher.find_drive_for_path(task_parent) {
                                    task_parent.strip_prefix(&drive.master_path).ok().map(|p| p.to_path_buf())
                                } else {
                                    None
                                };

                                if let Some(rel) = rel_parent {
                                    let sidecar_name = if is_dvd {
                                        doc.file_context.file_name.clone()
                                    } else {
                                        format!("{}.{}", task_stem, doc.file_context.extension)
                                    };
                                    resolved_rel_path = Some(rel.join(sidecar_name));
                                    break;
                                }
                            }
                        }
                    }
                }

                // Check 2: Companion in existing CanonicalMaster database catalog
                if resolved_rel_path.is_none() {
                    for master in &canonical_masters {
                        let master_stem = Path::new(&master.file_context.file_name)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or_default();
                        if UnifiedFileTransferService::is_companion_sidecar_stem_match(master_stem, stem, &doc.file_context.extension) {
                            let master_path = &master.file_context.file_path;
                            let rel_opt = if let Ok(rel) = master_path.strip_prefix(&target_drive.master_path) {
                                rel.parent().map(|p| p.to_path_buf())
                            } else if let Some(drive) = matcher.find_drive_for_path(master_path) {
                                master_path
                                    .strip_prefix(&drive.master_path)
                                    .ok()
                                    .and_then(|rel| rel.parent().map(|p| p.to_path_buf()))
                            } else {
                                None
                            };

                            if let Some(rel_parent) = rel_opt {
                                let sidecar_name = if is_dvd {
                                    doc.file_context.file_name.clone()
                                } else {
                                    format!("{}.{}", master_stem, doc.file_context.extension)
                                };
                                resolved_rel_path = Some(rel_parent.join(sidecar_name));
                                break;
                            }
                        }
                    }
                }

                // Check 3: Companion media file on disk beside this sidecar
                if resolved_rel_path.is_none() {
                    if let Some(comp_path) =
                        UnifiedFileTransferService::find_companion_media_file(&doc.file_context.file_path)
                    {
                        let comp_cat = crate::infrastructure::classifier::PhfMediaClassifier::new()
                            .classify(&comp_path)
                            .unwrap_or(if is_dvd { MediaCategory::Video } else { MediaCategory::Photo });
                        let comp_stem = comp_path
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or(stem);
                        let sidecar_name = if is_dvd {
                            doc.file_context.file_name.clone()
                        } else {
                            format!("{}.{}", comp_stem, doc.file_context.extension)
                        };
                        let cat_folder = UnifiedFileTransferService::category_folder_name(comp_cat);
                        let (y, date_folder) =
                            UnifiedFileTransferService::format_date_folder(year, month, day);
                        resolved_rel_path = Some(
                            PathBuf::from(format!("{}", y))
                                .join(date_folder)
                                .join(cat_folder)
                                .join(sidecar_name),
                        );
                    }
                }
            }

            let rel_path = if let Some(p) = resolved_rel_path {
                p
            } else {
                UnifiedFileTransferService::build_canonical_relative_path(
                    year,
                    month,
                    day,
                    cat,
                    &doc.file_context.file_name,
                )
            };

            // Resolve target volume across storage pool with capacity spillover
            let target_volume = {
                let required = doc
                    .file_context
                    .file_size_bytes
                    .saturating_add(self.min_free_space_buffer);
                let mut chosen = None;
                for vol in &pool_volumes {
                    let avail = volume_avail_space.get(&vol.id).copied().unwrap_or(0);
                    if avail >= required {
                        chosen = Some(*vol);
                        break;
                    }
                }
                match chosen {
                    Some(vol) => {
                        if let Some(space) = volume_avail_space.get_mut(&vol.id) {
                            *space = space.saturating_sub(doc.file_context.file_size_bytes);
                        }
                        vol
                    }
                    None => match self.resolve_pool_target_volume(
                        &pool_volumes,
                        doc.file_context.file_size_bytes,
                        self.disk_probe.as_ref(),
                        self.min_free_space_buffer,
                    ) {
                        Ok(vol) => vol,
                        Err(_) => target_drive,
                    },
                }
            };

            let initial_target_path = target_volume.master_path.join(&rel_path);

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
            source_drive_id,
            target_drive_id: target_drive.id.clone(),
            source_mount: source_mount_path,
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
        let plan = self.compile_plan(sink, config, &options).await?;
        self.execute_ingest_plan(sink, config, plan, options).await
    }

    /// Executes ingestion from a pre-compiled plan with interactive progress tracking.
    pub async fn execute_ingest_plan(
        &self,
        sink: &dyn MetadataStorageSink,
        config: &DriveConfiguration,
        plan: MasterIngestPlan,
        options: IngestOptions,
    ) -> Result<IngestExecutionReport, DeduplicationError> {
        let start = Instant::now();

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
                duplicates_linked: 0,
                replication_pending_count: 0,
                elapsed_seconds: start.elapsed().as_secs_f64(),
                errors: Vec::new(),
            });
        }

        let matcher = CanonicalPathMatcher::new(config.clone());
        let guard = ArchiveProtectionGuard::from_config(config);
        let mut files_transferred = 0usize;
        let mut files_skipped_identical = 0usize;
        let mut files_failed = 0usize;
        let mut bytes_transferred = 0u64;
        let mut source_files_deleted = 0usize;
        let mut errors = Vec::new();

        let total_tasks = plan.tasks.len();
        let is_tty = std::io::stderr().is_terminal();
        let pb = if is_tty {
            let p = indicatif::ProgressBar::new(plan.total_bytes);
            p.set_style(
                indicatif::ProgressStyle::default_bar()
                    .template("{spinner:.green} [{bar:28.cyan/blue}] {percent}% | {bytes}/{total_bytes} ({bytes_per_sec}) | Item {msg} | Elapsed: {elapsed} | ETA: {eta}")
                    .unwrap_or_else(|_| indicatif::ProgressStyle::default_bar())
                    .progress_chars("=> "),
            );
            p.enable_steady_tick(std::time::Duration::from_millis(150));
            Some(p)
        } else {
            None
        };

        let target_drive = DrivePruningService::resolve_drive(config, &plan.target_drive_id);
        let target_group = target_drive
            .as_ref()
            .map(|d| d.group.clone())
            .unwrap_or_else(|| "primary".to_string());

        let configured_replica_drives: Vec<String> = config
            .drives
            .iter()
            .filter(|d| {
                d.id != plan.target_drive_id
                    && (d.group.starts_with("replica")
                        || d.group.eq_ignore_ascii_case("secondary"))
            })
            .map(|d| d.id.clone())
            .collect();

        let mut source_updates: Vec<(Uuid, DeduplicationMetadata)> = Vec::new();
        let mut new_master_docs: Vec<MediaMetadataDocument> = Vec::new();
        let mut master_links: Vec<(String, Uuid, PathBuf)> = Vec::new();

        for (idx, task) in plan.tasks.iter().enumerate() {
            let fname = task
                .source_path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            if let Some(ref p) = pb {
                p.set_message(format!("{}/{} ({})", idx + 1, total_tasks, fname));
            } else {
                eprintln!(
                    "[{}/{}] Ingesting '{}' ({:.1} MB)...",
                    idx + 1,
                    total_tasks,
                    fname,
                    task.file_size_bytes as f64 / (1024.0 * 1024.0)
                );
            }

            match task.status {
                IngestTaskStatus::SkippedIdentical => {
                    files_skipped_identical += 1;
                    if let Some(ref p) = pb {
                        p.inc(task.file_size_bytes);
                    }

                    // Even when the primary media file already exists in master (SkippedIdentical),
                    // transfer the companion .AAE sidecar if it is present in the source folder!
                    let mut buffer_config = options.buffer_config;
                    if !options.verify_hash {
                        buffer_config.in_flight_hash = false;
                        buffer_config.recheck_disk_hash = false;
                    }
                    match UnifiedFileTransferService::transfer_companion_sidecar_if_present(
                        &task.source_path,
                        &task.target_path,
                        buffer_config,
                        options.delete_source,
                    )
                    .await
                    {
                        Ok(sidecar_res) => {
                            if sidecar_res.found && !sidecar_res.skipped_identical {
                                bytes_transferred += sidecar_res.bytes_transferred;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                source = %task.source_path.display(),
                                error = %e,
                                "Warning: Failed transferring companion .AAE sidecar for identical media file"
                            );
                        }
                    }

                    let new_master_id = Uuid::new_v4();
                    let now = Utc::now();
                    let replication_state = if configured_replica_drives.is_empty() {
                        MasterReplicationState {
                            status: ReplicationStatus::Replicated,
                            pending_drives: Vec::new(),
                            replicated_drives: Vec::new(),
                            last_synced_at: Some(now),
                        }
                    } else {
                        MasterReplicationState {
                            status: ReplicationStatus::Pending,
                            pending_drives: configured_replica_drives.clone(),
                            replicated_drives: Vec::new(),
                            last_synced_at: None,
                        }
                    };

                    let actual_target_drive = matcher.find_drive_for_path(&task.target_path);
                    let actual_drive_id = actual_target_drive
                        .map(|d| d.id.clone())
                        .unwrap_or_else(|| plan.target_drive_id.clone());
                    let actual_drive_group = actual_target_drive
                        .map(|d| d.group.clone())
                        .unwrap_or_else(|| target_group.clone());
                    let actual_scanned_root = actual_target_drive
                        .map(|d| d.master_path.clone())
                        .unwrap_or_else(|| plan.target_master_root.clone());

                    let master_meta = DeduplicationMetadata {
                        cluster_id: Uuid::new_v4(),
                        status: DeduplicationStatus::CanonicalMaster,
                        canonical_master_id: Some(new_master_id),
                        canonical_master_path: Some(task.target_path.clone()),
                        drive_id: Some(actual_drive_id),
                        drive_group: Some(actual_drive_group),
                        is_in_master_path: true,
                        reclaimable_bytes: 0,
                        evaluated_at: now,
                        replication: Some(replication_state),
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
                            d.file_context.scanned_root = actual_scanned_root;
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
                                scanned_root: actual_scanned_root,
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

                    let is_source_protected = guard.is_protected_path(&task.source_path);
                    let is_source_on_replica_master = matcher
                        .is_in_canonical_master(&task.source_path)
                        && matcher
                            .find_drive_for_path(&task.source_path)
                            .map(|d| d.group.starts_with("replica") || d.group.eq_ignore_ascii_case("secondary"))
                            .unwrap_or(false);

                    let mut source_status = if is_source_on_replica_master {
                        DeduplicationStatus::ReplicaKeep
                    } else if is_source_protected {
                        DeduplicationStatus::CanonicalMaster
                    } else {
                        DeduplicationStatus::MarkForRemoval
                    };

                    if options.delete_source && task.source_path.exists() {
                        if is_source_protected {
                            tracing::warn!(
                                source = %task.source_path.display(),
                                "Skipping source deletion: path resides within protected archive storage."
                            );
                        } else {
                            match guard.safe_delete_file(&task.source_path) {
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
                    }

                    let meta = DeduplicationMetadata {
                        cluster_id: Uuid::new_v4(),
                        status: source_status,
                        canonical_master_id: Some(new_master_id),
                        canonical_master_path: Some(task.target_path.clone()),
                        drive_id: Some(plan.source_drive_id.clone()),
                        drive_group: None,
                        is_in_master_path: is_source_protected,
                        reclaimable_bytes: if is_source_protected {
                            0
                        } else {
                            task.file_size_bytes
                        },
                        evaluated_at: now,
                        replication: None,
                    };
                    source_updates.push((task.source_doc_id, meta));
                }
                IngestTaskStatus::Pending => {
                    // Acquire exclusive spindle locks for source and target drives
                    let _spindle_guard = self
                        .spindle_locks
                        .acquire_transfer_locks(&task.source_path, &task.target_path)
                        .await;

                    let mut buffer_config = options.buffer_config;
                    if !options.verify_hash {
                        buffer_config.in_flight_hash = false;
                        buffer_config.recheck_disk_hash = false;
                    }

                    // Use shared UnifiedFileTransferService for verified streaming copy
                    match UnifiedFileTransferService::stream_copy_verified(
                        &task.source_path,
                        &task.target_path,
                        task.file_size_bytes,
                        task.cryptographic_hash.as_deref(),
                        buffer_config,
                    )
                    .await
                    {
                        Ok(copied_bytes) => {
                            files_transferred += 1;
                            bytes_transferred += copied_bytes;
                            if let Some(ref p) = pb {
                                p.inc(copied_bytes);
                            }

                            // Transfer companion .AAE sidecar if present
                            match UnifiedFileTransferService::transfer_companion_sidecar_if_present(
                                &task.source_path,
                                &task.target_path,
                                buffer_config,
                                options.delete_source,
                            )
                            .await
                            {
                                Ok(sidecar_res) => {
                                    if sidecar_res.found && !sidecar_res.skipped_identical {
                                        bytes_transferred += sidecar_res.bytes_transferred;
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!(
                                        source = %task.source_path.display(),
                                        error = %e,
                                        "Warning: Failed transferring companion .AAE sidecar"
                                    );
                                }
                            }

                            // Create canonical master document on primary
                            let new_master_id = Uuid::new_v4();
                            let now = Utc::now();
                            let replication_state = if configured_replica_drives.is_empty() {
                                MasterReplicationState {
                                    status: ReplicationStatus::Replicated,
                                    pending_drives: Vec::new(),
                                    replicated_drives: Vec::new(),
                                    last_synced_at: Some(now),
                                }
                            } else {
                                MasterReplicationState {
                                    status: ReplicationStatus::Pending,
                                    pending_drives: configured_replica_drives.clone(),
                                    replicated_drives: Vec::new(),
                                    last_synced_at: None,
                                }
                            };

                            let actual_target_drive = matcher.find_drive_for_path(&task.target_path);
                            let actual_drive_id = actual_target_drive
                                .map(|d| d.id.clone())
                                .unwrap_or_else(|| plan.target_drive_id.clone());
                            let actual_drive_group = actual_target_drive
                                .map(|d| d.group.clone())
                                .unwrap_or_else(|| target_group.clone());
                            let actual_scanned_root = actual_target_drive
                                .map(|d| d.master_path.clone())
                                .unwrap_or_else(|| plan.target_master_root.clone());

                            let master_meta = DeduplicationMetadata {
                                cluster_id: Uuid::new_v4(),
                                status: DeduplicationStatus::CanonicalMaster,
                                canonical_master_id: Some(new_master_id),
                                canonical_master_path: Some(task.target_path.clone()),
                                drive_id: Some(actual_drive_id),
                                drive_group: Some(actual_drive_group),
                                is_in_master_path: true,
                                reclaimable_bytes: 0,
                                evaluated_at: now,
                                replication: Some(replication_state),
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
                                    d.file_context.scanned_root = actual_scanned_root;
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
                                        scanned_root: actual_scanned_root,
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

                            // Check if source file is on a protected archive drive (primary or replica)
                            let is_source_protected = guard.is_protected_path(&task.source_path);
                            let is_source_on_replica_master = matcher
                                .is_in_canonical_master(&task.source_path)
                                && matcher
                                    .find_drive_for_path(&task.source_path)
                                    .map(|d| d.group.starts_with("replica") || d.group.eq_ignore_ascii_case("secondary"))
                                    .unwrap_or(false);

                            let mut source_status = if is_source_on_replica_master {
                                DeduplicationStatus::ReplicaKeep
                            } else if is_source_protected {
                                DeduplicationStatus::CanonicalMaster
                            } else {
                                DeduplicationStatus::MarkForRemoval
                            };

                            if options.delete_source && task.source_path.exists() {
                                if is_source_protected {
                                    tracing::warn!(
                                        source = %task.source_path.display(),
                                        "Skipping source deletion: path resides within protected archive storage (primary or replica)."
                                    );
                                } else {
                                    match guard.safe_delete_file(&task.source_path) {
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
                            }

                            let source_meta = DeduplicationMetadata {
                                cluster_id: Uuid::new_v4(),
                                status: source_status,
                                canonical_master_id: Some(new_master_id),
                                canonical_master_path: Some(task.target_path.clone()),
                                drive_id: Some(plan.source_drive_id.clone()),
                                drive_group: None,
                                is_in_master_path: is_source_protected,
                                reclaimable_bytes: if is_source_protected {
                                    0
                                } else {
                                    task.file_size_bytes
                                },
                                evaluated_at: now,
                                replication: None,
                            };
                            source_updates.push((task.source_doc_id, source_meta));
                        }
                        Err(e) => {
                            files_failed += 1;
                            if let Some(ref p) = pb {
                                p.inc(task.file_size_bytes);
                            }
                            errors.push(format!("{}", e));
                        }
                    }
                }
                IngestTaskStatus::CopiedVerified => {}
                IngestTaskStatus::Failed(ref err) => {
                    files_failed += 1;
                    if let Some(ref p) = pb {
                        p.inc(task.file_size_bytes);
                    }
                    errors.push(err.clone());
                }
            }
        }

        if let Some(ref p) = pb {
            p.finish_and_clear();
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

        let duplicates_linked = if !master_links.is_empty() {
            match sink.link_canonical_masters(&master_links).await {
                Ok(n) => n,
                Err(e) => {
                    errors.push(format!(
                        "Failed linking duplicate records to canonical master: {}",
                        e
                    ));
                    0
                }
            }
        } else {
            0
        };

        let replication_pending_count = if !configured_replica_drives.is_empty() {
            new_master_docs.len()
        } else {
            0
        };

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
            duplicates_linked,
            replication_pending_count,
            elapsed_seconds: start.elapsed().as_secs_f64(),
            errors,
        })
    }
}
