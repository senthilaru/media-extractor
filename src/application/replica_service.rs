use chrono::Utc;
use std::collections::HashSet;
use std::io::IsTerminal;
use std::time::Instant;
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

use crate::application::drive_pruner::DrivePruningService;
use crate::application::transfer_service::UnifiedFileTransferService;
use crate::domain::dedup_models::{
    DeduplicationMetadata, DeduplicationStatus, DriveConfiguration, DriveDefinition,
    IngestTaskStatus, MasterReplicationState, PathResolution, ReplicaSyncPlan, ReplicaSyncReport,
    ReplicaSyncTask, ReplicationStatus, TransferBufferConfig,
};
use crate::domain::errors::DeduplicationError;
use crate::domain::metadata_models::{FileContext, MediaMetadataDocument};
use crate::domain::ports::MetadataStorageSink;
use crate::infrastructure::blake3_hasher::Blake3MediaHasher;
use crate::infrastructure::disk_probe::{DiskSpaceProbe, PosixDiskSpaceProbe};
use crate::infrastructure::drive_spindle_lock::DriveSpindleLockManager;
use crate::infrastructure::drives_config::CanonicalPathMatcher;

/// Default minimum free-space safety buffer on replica storage pool volumes (5 GB).
pub const DEFAULT_MIN_FREE_SPACE_BUFFER_BYTES: u64 = 5 * 1024 * 1024 * 1024;

/// Runtime options for a replica synchronization run.
#[derive(Debug, Clone)]
pub struct ReplicaSyncOptions {
    pub target_replica_id: Option<String>,
    pub primary_drive_id: Option<String>,
    pub dry_run: bool,
    pub verify_hash: bool,
    pub buffer_config: TransferBufferConfig,
}

impl Default for ReplicaSyncOptions {
    fn default() -> Self {
        Self {
            target_replica_id: None,
            primary_drive_id: None,
            dry_run: false,
            verify_hash: true,
            buffer_config: TransferBufferConfig::default(),
        }
    }
}

/// Service orchestrating fanout replication of CanonicalMaster media assets
/// to all configured and mounted replica drives.
#[derive(Debug, Clone)]
pub struct ReplicaSyncService {
    spindle_locks: DriveSpindleLockManager,
    disk_probe: Arc<dyn DiskSpaceProbe>,
    min_free_space_buffer: u64,
}

impl Default for ReplicaSyncService {
    fn default() -> Self {
        Self {
            spindle_locks: DriveSpindleLockManager::default(),
            disk_probe: Arc::new(PosixDiskSpaceProbe::new()),
            min_free_space_buffer: DEFAULT_MIN_FREE_SPACE_BUFFER_BYTES,
        }
    }
}

impl ReplicaSyncService {
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

    /// Checks if a canonical master is already mirrored on ANY mounted volume of the replica pool on disk.
    pub fn is_replicated_in_pool(
        master_doc: &MediaMetadataDocument,
        replica_volumes: &[&DriveDefinition],
    ) -> Option<PathBuf> {
        let mut rel_paths = Vec::new();

        // 1. If file_path has canonical components <Year>/<DateFolder>/<Category>/<filename>, extract relative path
        let components: Vec<_> = master_doc
            .file_context
            .file_path
            .components()
            .map(|c| c.as_os_str())
            .collect();
        if components.len() >= 4 {
            let len = components.len();
            let mut rel = PathBuf::new();
            for c in &components[len - 4..] {
                rel.push(c);
            }
            rel_paths.push(rel);
        }

        // 2. Fallback to build_canonical_relative_path from metadata
        let (year, month, day) = if master_doc.temporal.year > 0 {
            (
                master_doc.temporal.year,
                master_doc.temporal.month,
                master_doc.temporal.day,
            )
        } else if let Some(mtime) = master_doc.file_context.modified_at {
            use chrono::Datelike;
            (mtime.year(), mtime.month(), mtime.day())
        } else {
            (2026, 1, 1)
        };

        rel_paths.push(UnifiedFileTransferService::build_canonical_relative_path(
            year,
            month,
            day,
            master_doc.file_context.media_category,
            &master_doc.file_context.file_name,
        ));

        for vol in replica_volumes {
            for rel in &rel_paths {
                let candidate_path = vol.master_path.join(rel);
                if candidate_path.exists() {
                    return Some(candidate_path);
                }
            }
        }
        None
    }

    /// Resolves primary drive and candidate replica drives from configuration.
    pub fn resolve_drives<'a>(
        config: &'a DriveConfiguration,
        primary_id: Option<&str>,
        target_replica_id: Option<&str>,
    ) -> Result<(&'a DriveDefinition, Vec<&'a DriveDefinition>), DeduplicationError> {
        let primary_drive = if let Some(p_id) = primary_id {
            DrivePruningService::resolve_drive(config, p_id)
                .or_else(|| {
                    config
                        .drives
                        .iter()
                        .find(|d| d.group.eq_ignore_ascii_case(p_id))
                })
                .ok_or_else(|| {
                    DeduplicationError::ConfigError(format!(
                        "Primary drive '{}' not found in configuration",
                        p_id
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
                None => candidates[0],
            }
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
                            || d.group.eq_ignore_ascii_case("secondary"))
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
        let mut replica_groups: Vec<(String, Vec<&DriveDefinition>)> = Vec::new();

        for &rep in &replica_drives {
            if rep.mount_path.exists() {
                active_replica_ids.push(rep.id.clone());
                if let Some(entry) = replica_groups.iter_mut().find(|(g, _)| g == &rep.group) {
                    entry.1.push(rep);
                } else {
                    replica_groups.push((rep.group.clone(), vec![rep]));
                }
            } else {
                offline_replica_ids.push(rep.id.clone());
            }
        }

        let canonical_masters = sink.query_canonical_masters().await?;
        let matcher = CanonicalPathMatcher::new(config.clone());
        let mut tasks = Vec::new();
        let mut total_bytes = 0u64;
        let hasher = Blake3MediaHasher::new();

        for (group_name, group_volumes) in replica_groups {
            // Query catalog records already present on ANY volume in this replica group across the storage pool
            // (including offline/unmounted volumes like SP-2 that are already cataloged in the database).
            // If the user explicitly targeted a specific replica drive with --replica, only check that target drive.
            let is_targeted_sync = options.target_replica_id.is_some();
            let pool_volume_candidates: Vec<&DriveDefinition> = if let Some(ref target_id) = options.target_replica_id {
                group_volumes.iter().copied().filter(|v| v.id == *target_id).collect()
            } else {
                let all_pool_volumes = config.get_pool_volumes(&group_name);
                if all_pool_volumes.is_empty() {
                    group_volumes.clone()
                } else {
                    all_pool_volumes
                }
            };

            let mut existing_hashes: HashSet<String> = HashSet::new();
            let mut existing_master_ids: HashSet<Uuid> = HashSet::new();
            for vol in &pool_volume_candidates {
                let existing_replicas = sink.query_replica_documents(&vol.id).await?;
                for doc in existing_replicas {
                    if let Some(ref f) = doc.fingerprint {
                        existing_hashes.insert(f.cryptographic_hash.clone());
                    }
                    if let Some(master_id) =
                        doc.dedup.as_ref().and_then(|meta| meta.canonical_master_id)
                    {
                        existing_master_ids.insert(master_id);
                    }
                }
            }

            // Track volume available space for capacity spillover across volumes in this replica group
            let mut volume_avail_space: std::collections::HashMap<String, u64> =
                std::collections::HashMap::new();
            for vol in &group_volumes {
                let avail = if vol.mount_path.exists() {
                    self.disk_probe
                        .available_space_bytes(&vol.mount_path)
                        .unwrap_or(0)
                } else {
                    0
                };
                volume_avail_space.insert(vol.id.clone(), avail);
            }

            for master in &canonical_masters {
                // If master file does not physically exist on disk (e.g. source master drive is offline/unmounted),
                // it cannot be replicated during this run.
                if !master.file_context.file_path.exists() {
                    continue;
                }

                let master_hash = master
                    .fingerprint
                    .as_ref()
                    .map(|f| f.cryptographic_hash.clone());

                let is_already_registered = existing_master_ids.contains(&master.id)
                    || master_hash
                        .as_ref()
                        .map(|h| existing_hashes.contains(h))
                        .unwrap_or(false)
                    || (!is_targeted_sync
                        && master
                            .dedup
                            .as_ref()
                            .and_then(|d| d.replication.as_ref())
                            .map(|rep| {
                                rep.replicated_drives.iter().any(|d_id| {
                                    pool_volume_candidates.iter().any(|v| v.id == *d_id)
                                })
                            })
                            .unwrap_or(false));

                if is_already_registered {
                    continue; // Already replicated and cataloged on this replica group
                }

                // Also check if the physical file already exists on ANY volume of this replica pool
                if Self::is_replicated_in_pool(master, &group_volumes).is_some() {
                    continue;
                }

                // Derive canonical relative path
                let rel_path = if let Some(drive) =
                    matcher.find_drive_for_path(&master.file_context.file_path)
                {
                    master
                        .file_context
                        .file_path
                        .strip_prefix(&drive.master_path)
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|_| {
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
                        })
                } else {
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
                };

                // Select target volume in replica group using capacity spillover
                let target_vol = {
                    let required = master
                        .file_context
                        .file_size_bytes
                        .saturating_add(self.min_free_space_buffer);
                    let mut chosen = None;
                    for vol in &group_volumes {
                        let avail = volume_avail_space.get(&vol.id).copied().unwrap_or(0);
                        if avail >= required {
                            chosen = Some(*vol);
                            break;
                        }
                    }
                    match chosen {
                        Some(vol) => {
                            if let Some(space) = volume_avail_space.get_mut(&vol.id) {
                                *space =
                                    space.saturating_sub(master.file_context.file_size_bytes);
                            }
                            vol
                        }
                        None => group_volumes[0],
                    }
                };

                let target_path = target_vol.master_path.join(&rel_path);

                let resolution = UnifiedFileTransferService::resolve_collision_path(
                    &target_path,
                    master.file_context.file_size_bytes,
                    master_hash.as_deref(),
                    &hasher,
                );

                let (dest_path, status) = match resolution {
                    PathResolution::IdenticalExists(p) => {
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
                    replica_drive_id: target_vol.id.clone(),
                    replica_group: target_vol.group.clone(),
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
        let plan = self.compile_plan(sink, config, &options).await?;
        self.execute_sync_plan(sink, config, plan, options).await
    }

    /// Executes replication from a pre-compiled plan with interactive progress tracking.
    pub async fn execute_sync_plan(
        &self,
        sink: &dyn MetadataStorageSink,
        config: &DriveConfiguration,
        plan: ReplicaSyncPlan,
        options: ReplicaSyncOptions,
    ) -> Result<ReplicaSyncReport, DeduplicationError> {
        let start = Instant::now();

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
        let mut successful_replications: Vec<(Uuid, String)> = Vec::new();

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

        for (idx, task) in plan.tasks.iter().enumerate() {
            let fname = task
                .source_path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            if let Some(ref p) = pb {
                p.set_message(format!(
                    "{}/{} ({} -> {})",
                    idx + 1,
                    total_tasks,
                    fname,
                    task.replica_drive_id
                ));
            } else {
                eprintln!(
                    "[{}/{}] Replicating '{}' to {} ({:.1} MB)...",
                    idx + 1,
                    total_tasks,
                    fname,
                    task.replica_drive_id,
                    task.file_size_bytes as f64 / (1024.0 * 1024.0)
                );
            }

            match task.status {
                IngestTaskStatus::SkippedIdentical => {
                    files_skipped_identical += 1;
                    if let Some(ref p) = pb {
                        p.inc(task.file_size_bytes);
                    }
                    successful_replications
                        .push((task.canonical_master_id, task.replica_drive_id.clone()));
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
                        replication: None,
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
                            visual: None,
                        },
                    };
                    new_replica_docs.push(replica_doc);
                }
                IngestTaskStatus::Pending => {
                    // Acquire exclusive spindle locks for source (primary) and target (replica) drives
                    let _spindle_guard = self
                        .spindle_locks
                        .acquire_transfer_locks(&task.source_path, &task.target_path)
                        .await;

                    let mut buffer_config = options.buffer_config;
                    if !options.verify_hash {
                        buffer_config.in_flight_hash = false;
                        buffer_config.recheck_disk_hash = false;
                    }

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
                            files_replicated += 1;
                            bytes_transferred += copied_bytes;
                            if let Some(ref p) = pb {
                                p.inc(copied_bytes);
                            }

                            // If a companion .AAE sidecar exists on primary master, replicate it alongside the media file
                            match UnifiedFileTransferService::transfer_companion_sidecar_if_present(
                                &task.source_path,
                                &task.target_path,
                                buffer_config,
                                false, // Never delete from primary master
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
                                        "Warning: Failed replicating companion .AAE sidecar"
                                    );
                                }
                            }
                            successful_replications
                                .push((task.canonical_master_id, task.replica_drive_id.clone()));

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
                                replication: None,
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
                                    visual: None,
                                },
                            };
                            new_replica_docs.push(replica_doc);
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

        if !new_replica_docs.is_empty() {
            if let Err(e) = sink.save_batch(&new_replica_docs).await {
                errors.push(format!("Failed saving replica records to database: {}", e));
            }
        }

        // Update MasterReplicationState for each synchronized master
        for (master_id, replica_drive_id) in &successful_replications {
            if let Ok(Some(master_doc)) = sink.find_document_by_id(*master_id).await {
                let mut rep_state = master_doc
                    .dedup
                    .as_ref()
                    .and_then(|d| d.replication.clone())
                    .unwrap_or_else(|| MasterReplicationState {
                        status: ReplicationStatus::Pending,
                        pending_drives: Vec::new(),
                        replicated_drives: Vec::new(),
                        last_synced_at: None,
                    });

                // Remove all drives in the same replica pool group from pending_drives
                let rep_drive = config.drives.iter().find(|d| d.id == *replica_drive_id);
                if let Some(drive) = rep_drive {
                    let pool_volumes = config.get_pool_volumes(&drive.group);
                    let pool_ids: std::collections::HashSet<&str> =
                        pool_volumes.iter().map(|v| v.id.as_str()).collect();
                    rep_state.pending_drives.retain(|d| !pool_ids.contains(d.as_str()));
                } else {
                    rep_state.pending_drives.retain(|d| d != replica_drive_id);
                }

                if !rep_state.replicated_drives.contains(replica_drive_id) {
                    rep_state.replicated_drives.push(replica_drive_id.clone());
                }
                rep_state.last_synced_at = Some(Utc::now());
                if rep_state.pending_drives.is_empty() {
                    rep_state.status = ReplicationStatus::Replicated;
                } else {
                    rep_state.status = ReplicationStatus::PartiallyReplicated;
                }

                if let Err(e) = sink
                    .update_master_replication_state(*master_id, &rep_state)
                    .await
                {
                    errors.push(format!(
                        "Failed updating replication state for master {}: {}",
                        master_id, e
                    ));
                }
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
