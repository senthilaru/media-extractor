use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::domain::dedup_models::{
    DeduplicationStatus, DriveConfiguration, DriveDefinition, DriveRemovalManifest,
    DriveRemovalTask,
};
use crate::domain::errors::DeduplicationError;
use crate::domain::ports::{MediaFingerprinter, MetadataStorageSink};
use crate::infrastructure::blake3_hasher::Blake3MediaHasher;

/// Execution report detailing the outcome of a drive pruning run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PruneExecutionReport {
    pub drive_id: String,
    pub mount_path: PathBuf,
    pub is_dry_run: bool,
    pub total_candidates: usize,
    pub verified_masters: usize,
    pub unverified_masters: usize,
    pub files_deleted: usize,
    pub bytes_reclaimed: u64,
    pub errors: Vec<String>,
}

/// Options configuring a drive pruning operation.
#[derive(Debug, Clone)]
pub struct DrivePrunerOptions {
    pub drive_identifier: String,
    pub execute: bool,
    pub require_master_disk: bool,
    pub manifest_out: Option<PathBuf>,
}

/// Service orchestrating drive-by-drive duplicate pruning with pre-deletion master verification.
#[derive(Debug, Default, Clone)]
pub struct DrivePruningService;

impl DrivePruningService {
    pub fn new() -> Self {
        Self
    }

    /// Resolves a drive from `DriveConfiguration` by ID or mount path prefix.
    pub fn resolve_drive<'a>(
        config: &'a DriveConfiguration,
        identifier: &str,
    ) -> Option<&'a DriveDefinition> {
        let trimmed = identifier.trim();
        // Exact match by ID
        if let Some(drive) = config.drives.iter().find(|d| d.id == trimmed) {
            return Some(drive);
        }

        // Match by mount path
        let ident_path = Path::new(trimmed);
        config.drives.iter().find(|d| {
            d.mount_path == ident_path
                || trimmed.starts_with(&d.mount_path.to_string_lossy().to_string())
        })
    }

    /// Resolves a drive from `DriveConfiguration` or dynamically synthesizes one from an
    /// external drive mount path or local directory path without requiring pre-configuration.
    pub fn resolve_drive_or_path(
        config: &DriveConfiguration,
        identifier: &str,
    ) -> Result<DriveDefinition, DeduplicationError> {
        if let Some(drive) = Self::resolve_drive(config, identifier) {
            return Ok(drive.clone());
        }

        let trimmed = identifier.trim();
        let trimmed_path =
            if trimmed.len() > 1 && (trimmed.ends_with('/') || trimmed.ends_with('\\')) {
                trimmed.trim_end_matches(['/', '\\'])
            } else {
                trimmed
            };

        let path = PathBuf::from(trimmed_path);
        if !path.exists() {
            return Err(DeduplicationError::DriveNotMounted {
                drive: trimmed.to_string(),
                path: path.clone(),
            });
        }

        let id = path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(trimmed_path)
            .to_string();

        Ok(DriveDefinition {
            id,
            group: "source".to_string(),
            mount_path: path.clone(),
            master_path: path,
        })
    }

    /// Generates a removal manifest for the specified drive by querying MongoDB.
    pub async fn generate_manifest(
        &self,
        sink: &dyn MetadataStorageSink,
        config: &DriveConfiguration,
        drive_identifier: &str,
        require_master_disk: bool,
    ) -> Result<DriveRemovalManifest, DeduplicationError> {
        let drive = Self::resolve_drive_or_path(config, drive_identifier)?;

        let mount_str = drive.mount_path.to_string_lossy();
        let candidates = sink.query_removal_candidates(Some(&mount_str)).await?;

        // Preload canonical masters to verify masters by ID or by bitwise cryptographic hash
        let canonical_masters = sink.query_canonical_masters().await.unwrap_or_default();
        let mut master_by_id: std::collections::HashMap<
            uuid::Uuid,
            &crate::domain::metadata_models::MediaMetadataDocument,
        > = std::collections::HashMap::new();
        let mut master_by_hash: std::collections::HashMap<
            String,
            &crate::domain::metadata_models::MediaMetadataDocument,
        > = std::collections::HashMap::new();
        for m in &canonical_masters {
            master_by_id.insert(m.id, m);
            if let Some(ref fp) = m.fingerprint {
                master_by_hash.insert(fp.cryptographic_hash.clone(), m);
            }
        }

        let mut tasks = Vec::with_capacity(candidates.len());
        let mut total_reclaimable_bytes = 0u64;

        for doc in candidates {
            let dedup = match doc.dedup {
                Some(d) => d,
                None => continue,
            };

            let mut master_path = dedup.canonical_master_path.clone().unwrap_or_default();
            let mut master_verified = false;

            // 1. Verify master in database by canonical_master_id
            if let Some(master_id) = dedup.canonical_master_id {
                if let Some(master_doc) = master_by_id.get(&master_id) {
                    master_verified = true;
                    if master_path.as_os_str().is_empty() {
                        master_path = master_doc.file_context.file_path.clone();
                    }
                } else if let Ok(Some(master_doc)) = sink.find_document_by_id(master_id).await {
                    if let Some(m_dedup) = master_doc.dedup {
                        if m_dedup.status == DeduplicationStatus::CanonicalMaster
                            || m_dedup.status == DeduplicationStatus::ReplicaKeep
                        {
                            master_verified = true;
                            if master_path.as_os_str().is_empty() {
                                master_path = master_doc.file_context.file_path;
                            }
                        }
                    } else {
                        master_verified = true;
                        if master_path.as_os_str().is_empty() {
                            master_path = master_doc.file_context.file_path;
                        }
                    }
                }
            }

            // 2. Fallback: Verify master by bitwise cryptographic hash if unlinked
            if !master_verified {
                if let Some(ref fp) = doc.fingerprint {
                    if let Some(master_doc) = master_by_hash.get(&fp.cryptographic_hash) {
                        master_verified = true;
                        if master_path.as_os_str().is_empty() {
                            master_path = master_doc.file_context.file_path.clone();
                        }
                    }
                }
            }

            // 3. Fallback: Verify master directly on disk at canonical_master_path
            if !master_verified && !master_path.as_os_str().is_empty() && master_path.exists() {
                if let Ok(meta) = std::fs::metadata(&master_path) {
                    if meta.len() == doc.file_context.file_size_bytes {
                        if let Some(ref fp) = doc.fingerprint {
                            let hasher = Blake3MediaHasher::new();
                            if let Ok(h) = hasher.compute_cryptographic_hash(&master_path) {
                                if h == fp.cryptographic_hash {
                                    master_verified = true;
                                }
                            }
                        } else {
                            master_verified = true;
                        }
                    }
                }
            }

            // 4. Fallback: If master_path is empty, check canonical path on primary drive
            if !master_verified {
                if let Some(primary_drive) = config
                    .drives
                    .iter()
                    .find(|d| d.group.eq_ignore_ascii_case("primary"))
                {
                    let cat = doc.file_context.media_category;
                    let (year, month, day) = if doc.temporal.year > 0 {
                        (doc.temporal.year, doc.temporal.month, doc.temporal.day)
                    } else if let Some(mtime) = doc.file_context.modified_at {
                        use chrono::Datelike;
                        (mtime.year(), mtime.month(), mtime.day())
                    } else {
                        (2026, 1, 1)
                    };
                    let rel_path = crate::application::transfer_service::UnifiedFileTransferService::build_canonical_relative_path(
                        year,
                        month,
                        day,
                        cat,
                        &doc.file_context.file_name,
                    );
                    let mut candidate_paths = vec![primary_drive.master_path.join(&rel_path)];

                    // Also try normalized name if file_name contains duplicate copy suffixes
                    let fname = &doc.file_context.file_name;
                    if fname.contains(" copy") {
                        let base_name = fname.replace(" copy", "");
                        let rel_base = crate::application::transfer_service::UnifiedFileTransferService::build_canonical_relative_path(
                            year,
                            month,
                            day,
                            cat,
                            &base_name,
                        );
                        candidate_paths.push(primary_drive.master_path.join(&rel_base));
                    }

                    for candidate_master_path in candidate_paths {
                        if candidate_master_path.exists() {
                            if let Ok(meta) = std::fs::metadata(&candidate_master_path) {
                                if meta.len() == doc.file_context.file_size_bytes {
                                    if let Some(ref fp) = doc.fingerprint {
                                        let hasher = Blake3MediaHasher::new();
                                        if let Ok(h) = hasher
                                            .compute_cryptographic_hash(&candidate_master_path)
                                        {
                                            if h == fp.cryptographic_hash {
                                                master_verified = true;
                                                master_path = candidate_master_path;
                                                break;
                                            }
                                        }
                                    } else {
                                        master_verified = true;
                                        master_path = candidate_master_path;
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // 5. If required, verify master exists on mounted physical disk
            if master_verified
                && require_master_disk
                && !master_path.as_os_str().is_empty()
                && !master_path.exists()
            {
                master_verified = false;
            }

            let task = DriveRemovalTask {
                document_id: doc.id,
                target_file_path: doc.file_context.file_path,
                canonical_master_path: master_path,
                file_size_bytes: doc.file_context.file_size_bytes,
                master_exists_verified: master_verified,
            };

            total_reclaimable_bytes += task.file_size_bytes;
            tasks.push(task);
        }

        Ok(DriveRemovalManifest {
            drive_id: drive.id.clone(),
            mount_path: drive.mount_path.clone(),
            drive_group: drive.group.clone(),
            total_files_to_remove: tasks.len(),
            total_reclaimable_bytes,
            removal_tasks: tasks,
        })
    }

    /// Prunes marked duplicates on disk and updates database records.
    pub async fn prune_drive(
        &self,
        sink: &dyn MetadataStorageSink,
        config: &DriveConfiguration,
        options: DrivePrunerOptions,
    ) -> Result<PruneExecutionReport, DeduplicationError> {
        let manifest = self
            .generate_manifest(
                sink,
                config,
                &options.drive_identifier,
                options.require_master_disk,
            )
            .await?;

        // If manifest output requested, write to disk
        if let Some(ref out_path) = options.manifest_out {
            let json = serde_json::to_string_pretty(&manifest).map_err(|e| {
                DeduplicationError::ProcessingFailed(format!("Failed to serialize manifest: {}", e))
            })?;
            std::fs::write(out_path, json).map_err(|e| DeduplicationError::PruneIoError {
                path: out_path.clone(),
                source: e,
            })?;
        }

        let mut verified_count = 0usize;
        let mut unverified_count = 0usize;
        let mut deleted_count = 0usize;
        let mut bytes_reclaimed = 0u64;
        let mut errors = Vec::new();

        for task in &manifest.removal_tasks {
            if !task.master_exists_verified {
                unverified_count += 1;
                errors.push(format!(
                    "Skipped unverified master for '{}'",
                    task.target_file_path.display()
                ));
                continue;
            }

            verified_count += 1;

            if options.execute {
                if task.target_file_path.exists() {
                    match std::fs::remove_file(&task.target_file_path) {
                        Ok(_) => {
                            deleted_count += 1;
                            bytes_reclaimed += task.file_size_bytes;
                            if let Err(e) = sink.mark_removed(task.document_id).await {
                                errors.push(format!(
                                    "Failed to update status for '{}': {}",
                                    task.target_file_path.display(),
                                    e
                                ));
                            }
                        }
                        Err(e) => {
                            errors.push(format!(
                                "Failed to delete file '{}': {}",
                                task.target_file_path.display(),
                                e
                            ));
                        }
                    }
                } else {
                    // File already gone from disk, update database record
                    deleted_count += 1;
                    bytes_reclaimed += task.file_size_bytes;
                    let _ = sink.mark_removed(task.document_id).await;
                }
            } else {
                // Dry-run mode: count as would-be reclaimed
                bytes_reclaimed += task.file_size_bytes;
            }
        }

        Ok(PruneExecutionReport {
            drive_id: manifest.drive_id,
            mount_path: manifest.mount_path,
            is_dry_run: !options.execute,
            total_candidates: manifest.total_files_to_remove,
            verified_masters: verified_count,
            unverified_masters: unverified_count,
            files_deleted: deleted_count,
            bytes_reclaimed,
            errors,
        })
    }
}
