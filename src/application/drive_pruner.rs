use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::application::transfer_service::UnifiedFileTransferService;
use crate::domain::dedup_models::{
    DeduplicationStatus, DriveConfiguration, DriveDefinition, DriveRemovalManifest,
    DriveRemovalTask,
};
use crate::domain::errors::DeduplicationError;
use crate::domain::ports::{MediaFingerprinter, MetadataStorageSink};
use crate::infrastructure::archive_guard::ArchiveProtectionGuard;
use crate::infrastructure::blake3_hasher::Blake3MediaHasher;

/// Recognized disposable operating system indexing files that may linger in vacated folders.
pub fn is_disposable_os_file(name: &str) -> bool {
    matches!(
        name,
        ".DS_Store" | "Thumbs.db" | "ehthumbs.db" | ".Spotlight-V100" | ".Trashes"
    )
}

/// Inspects a directory to determine if it is empty or contains only disposable OS files.
/// If `execute` is true, disposable OS files within an otherwise empty directory are unlinked.
fn check_directory_empty(dir: &Path, execute: bool) -> Result<bool, std::io::Error> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err),
    };

    let mut disposable_files = Vec::new();
    for entry in entries {
        let entry = entry?;
        let file_name = entry.file_name();
        let name_str = file_name.to_string_lossy();
        if is_disposable_os_file(&name_str) {
            disposable_files.push(entry.path());
        } else {
            return Ok(false);
        }
    }

    if execute {
        for f in disposable_files {
            let _ = std::fs::remove_file(f);
        }
    }
    Ok(true)
}

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
    pub records_purged: usize,
    pub bytes_reclaimed: u64,
    #[serde(default)]
    pub empty_directories_removed: usize,
    #[serde(default)]
    pub empty_directories_eligible: usize,
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

        // Match by mount path (exact mount path match)
        let trimmed_path = trimmed.trim_end_matches(['/', '\\']);
        let ident_path = Path::new(trimmed_path);
        let canonical_ident = ArchiveProtectionGuard::canonicalize_safe(ident_path);

        config.drives.iter().find(|d| {
            let canonical_mount = ArchiveProtectionGuard::canonicalize_safe(&d.mount_path);
            canonical_ident == canonical_mount
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
        let guard = ArchiveProtectionGuard::from_config(config);

        // Layer 1: Fail fast if target drive identifier or path corresponds to protected archive
        guard.validate_prune_target(drive_identifier, config)?;

        let drive = Self::resolve_drive_or_path(config, drive_identifier)?;
        ArchiveProtectionGuard::validate_drive_definition(&drive)?;

        if guard.is_protected_path(&drive.mount_path) {
            return Err(DeduplicationError::PruneForbiddenOnProtectedArchive {
                drive: drive.id.clone(),
                path: drive.mount_path.clone(),
                reason:
                    "Pruning is strictly forbidden on protected canonical master archive storage."
                        .to_string(),
            });
        }

        let mount_str = drive.mount_path.to_string_lossy();
        let candidates = sink.query_removal_candidates(Some(&mount_str)).await?;

        // Preload canonical masters to verify masters by ID, hash, or canonical path
        let canonical_masters = sink.query_canonical_masters().await.unwrap_or_default();
        let mut master_by_id: std::collections::HashMap<
            uuid::Uuid,
            &crate::domain::metadata_models::MediaMetadataDocument,
        > = std::collections::HashMap::new();
        let mut master_by_hash: std::collections::HashMap<
            String,
            &crate::domain::metadata_models::MediaMetadataDocument,
        > = std::collections::HashMap::new();
        let mut master_by_path: std::collections::HashMap<
            std::path::PathBuf,
            &crate::domain::metadata_models::MediaMetadataDocument,
        > = std::collections::HashMap::new();
        for m in &canonical_masters {
            master_by_id.insert(m.id, m);
            if let Some(ref fp) = m.fingerprint {
                master_by_hash.insert(fp.cryptographic_hash.clone(), m);
            }
            master_by_path.insert(m.file_context.file_path.clone(), m);
        }

        let mut tasks = Vec::with_capacity(candidates.len());
        let mut total_reclaimable_bytes = 0u64;

        for doc in candidates {
            // Layer 2: Disqualify candidate if its physical path resides inside protected archive
            if guard.is_protected_path(&doc.file_context.file_path) {
                tracing::warn!(
                    path = %doc.file_context.file_path.display(),
                    "Disqualifying candidate from removal manifest: target resides inside protected archive storage"
                );
                continue;
            }

            let dedup = match doc.dedup {
                Some(d) => d,
                None => continue,
            };

            let mut master_path = dedup.canonical_master_path.clone().unwrap_or_default();
            let mut master_verified = false;

            let is_sidecar = doc.file_context.media_category
                == crate::domain::models::MediaCategory::Sidecar
                || doc.file_context.extension.eq_ignore_ascii_case("aae")
                || doc.file_context.extension.eq_ignore_ascii_case("xmp");

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

            // 2b. Fallback: Verify master by canonical path in database
            if !master_verified && !master_path.as_os_str().is_empty() {
                if let Some(master_doc) = master_by_path.get(&master_path) {
                    if is_sidecar {
                        master_verified = true;
                    } else if let (Some(ref m_fp), Some(ref doc_fp)) =
                        (&master_doc.fingerprint, &doc.fingerprint)
                    {
                        if m_fp.cryptographic_hash == doc_fp.cryptographic_hash {
                            master_verified = true;
                        }
                    } else if master_doc.file_context.file_size_bytes
                        == doc.file_context.file_size_bytes
                    {
                        master_verified = true;
                    }
                }
            }

            // 3. Fallback: Verify master directly on disk at canonical_master_path
            if !master_verified && !master_path.as_os_str().is_empty() && master_path.exists() {
                if let Ok(meta) = std::fs::metadata(&master_path) {
                    if is_sidecar && guard.is_protected_path(&master_path) {
                        let companion_in_master =
                            UnifiedFileTransferService::find_companion_media_file(&master_path)
                                .is_some();
                        if companion_in_master || meta.len() > 0 {
                            master_verified = true;
                        }
                    } else if meta.len() == doc.file_context.file_size_bytes {
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

            // 4. Fallback: If master_path is unverified or empty, check canonical path on mounted master archive drives
            if !master_verified {
                let master_drives: Vec<&DriveDefinition> = config
                    .drives
                    .iter()
                    .filter(|d| {
                        (d.group.eq_ignore_ascii_case("primary")
                            || d.group.eq_ignore_ascii_case("secondary")
                            || d.group.starts_with("replica"))
                            && (d.master_path.exists() || d.mount_path.exists())
                    })
                    .collect();

                let cat = doc.file_context.media_category;
                let (year, month, day) = if doc.temporal.year > 0 {
                    (doc.temporal.year, doc.temporal.month, doc.temporal.day)
                } else if let Some(mtime) = doc.file_context.modified_at {
                    use chrono::Datelike;
                    (mtime.year(), mtime.month(), mtime.day())
                } else {
                    (2026, 1, 1)
                };

                for master_drive in master_drives {
                    let rel_path =
                        UnifiedFileTransferService::build_canonical_relative_path(
                            year,
                            month,
                            day,
                            cat,
                            &doc.file_context.file_name,
                        );
                    let mut candidate_paths = vec![master_drive.master_path.join(&rel_path)];

                    // Also try normalized name if file_name contains duplicate copy suffixes
                    let fname = &doc.file_context.file_name;
                    if fname.contains(" copy") {
                        let base_name = fname.replace(" copy", "");
                        let rel_base =
                            UnifiedFileTransferService::build_canonical_relative_path(
                                year,
                                month,
                                day,
                                cat,
                                &base_name,
                            );
                        candidate_paths.push(master_drive.master_path.join(&rel_base));
                    }

                    if is_sidecar {
                        let ext = doc.file_context.extension.to_ascii_lowercase();
                        let stem = std::path::Path::new(fname)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or(fname);

                        for e in &[ext.as_str(), "aae", "AAE", "xmp", "XMP"] {
                            let rel_sidecar =
                                UnifiedFileTransferService::build_canonical_relative_path(
                                    year,
                                    month,
                                    day,
                                    cat,
                                    &format!("{}.{}", stem, e),
                                );
                            candidate_paths.push(master_drive.master_path.join(&rel_sidecar));

                            if stem.starts_with("IMG_O") {
                                let norm_stem = format!("IMG_{}", &stem[5..]);
                                let rel_norm =
                                    UnifiedFileTransferService::build_canonical_relative_path(
                                        year,
                                        month,
                                        day,
                                        cat,
                                        &format!("{}.{}", norm_stem, e),
                                    );
                                candidate_paths.push(master_drive.master_path.join(&rel_norm));
                            } else if stem.starts_with("IMG_") {
                                let o_stem = format!("IMG_O{}", &stem[4..]);
                                let rel_o =
                                    UnifiedFileTransferService::build_canonical_relative_path(
                                        year,
                                        month,
                                        day,
                                        cat,
                                        &format!("{}.{}", o_stem, e),
                                    );
                                candidate_paths.push(master_drive.master_path.join(&rel_o));
                            }
                        }
                    }

                    for candidate_master_path in candidate_paths {
                        if candidate_master_path.exists()
                            && guard.is_protected_path(&candidate_master_path)
                        {
                            if is_sidecar {
                                let companion_exists =
                                    UnifiedFileTransferService::find_companion_media_file(
                                        &candidate_master_path,
                                    )
                                    .is_some();
                                if companion_exists
                                    || std::fs::metadata(&candidate_master_path)
                                        .map(|m| m.len() > 0)
                                        .unwrap_or(false)
                                {
                                    master_verified = true;
                                    master_path = candidate_master_path;
                                    break;
                                }
                            } else if let Ok(meta) = std::fs::metadata(&candidate_master_path) {
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

                    if master_verified {
                        break;
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
        let guard = ArchiveProtectionGuard::from_config(config);
        guard.validate_prune_target(&options.drive_identifier, config)?;

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
        let mut records_purged = 0usize;
        let mut bytes_reclaimed = 0u64;
        let mut errors = Vec::new();
        let mut affected_file_paths = Vec::new();

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
                // Layer 3: Runtime circuit breaker
                if guard.is_protected_path(&task.target_file_path) {
                    errors.push(format!(
                        "CRITICAL: Refused deletion of protected archive file '{}'",
                        task.target_file_path.display()
                    ));
                    continue;
                }

                if task.target_file_path.exists() {
                    match guard.safe_delete_file(&task.target_file_path) {
                        Ok(_) => {
                            deleted_count += 1;
                            bytes_reclaimed += task.file_size_bytes;
                            affected_file_paths.push(task.target_file_path.clone());

                            // Check if a companion .AAE sidecar exists in the source folder
                            if let Some(source_sidecar) =
                                UnifiedFileTransferService::find_companion_sidecar(
                                    &task.target_file_path,
                                )
                            {
                                let master_has_sidecar =
                                    UnifiedFileTransferService::find_companion_sidecar(
                                        &task.canonical_master_path,
                                    )
                                    .is_some();
                                if master_has_sidecar {
                                    let sidecar_size = std::fs::metadata(&source_sidecar)
                                        .map(|m| m.len())
                                        .unwrap_or(0);
                                    if guard.safe_delete_file(&source_sidecar).is_ok() {
                                        bytes_reclaimed += sidecar_size;
                                        affected_file_paths.push(source_sidecar);
                                    }
                                }
                            }

                            match sink.delete_document(task.document_id).await {
                                Ok(true) => {
                                    records_purged += 1;
                                }
                                Ok(false) => {}
                                Err(e) => {
                                    errors.push(format!(
                                        "Failed to purge database record for '{}': {}",
                                        task.target_file_path.display(),
                                        e
                                    ));
                                }
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
                } else if !guard.is_protected_path(&task.target_file_path) {
                    // File already gone from disk, purge obsolete database record
                    deleted_count += 1;
                    bytes_reclaimed += task.file_size_bytes;
                    affected_file_paths.push(task.target_file_path.clone());
                    if let Ok(true) = sink.delete_document(task.document_id).await {
                        records_purged += 1;
                    }
                }
            } else {
                // Dry-run mode: count as would-be reclaimed and would-be purged
                bytes_reclaimed += task.file_size_bytes;
                records_purged += 1;
                affected_file_paths.push(task.target_file_path.clone());

                if let Some(source_sidecar) =
                    UnifiedFileTransferService::find_companion_sidecar(&task.target_file_path)
                {
                    let sidecar_size = std::fs::metadata(&source_sidecar)
                        .map(|m| m.len())
                        .unwrap_or(0);
                    bytes_reclaimed += sidecar_size;
                    affected_file_paths.push(source_sidecar);
                }
            }
        }

        // Clean up empty ancestor directories
        let (empty_dirs_count, dir_errors) = Self::clean_empty_ancestor_directories(
            &manifest.mount_path,
            &affected_file_paths,
            &guard,
            options.execute,
        );
        errors.extend(dir_errors);

        // Purge any pre-existing records on target drive currently marked "Removed"
        if options.execute {
            let mount_str = manifest.mount_path.to_string_lossy();
            match sink.purge_removed_records(Some(&mount_str)).await {
                Ok(cleaned) => {
                    records_purged += cleaned;
                }
                Err(e) => {
                    errors.push(format!(
                        "Failed purging historical removed records on '{}': {}",
                        manifest.mount_path.display(),
                        e
                    ));
                }
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
            records_purged,
            bytes_reclaimed,
            empty_directories_removed: if options.execute { empty_dirs_count } else { 0 },
            empty_directories_eligible: if !options.execute {
                empty_dirs_count
            } else {
                0
            },
            errors,
        })
    }

    /// Cleans up empty ancestor directories bottom-up starting from the parent directories of
    /// deleted duplicate files, stopping at the target prune root.
    ///
    /// Guarantees:
    /// - `prune_root` itself is NEVER deleted.
    /// - Protected canonical master archive paths are NEVER deleted.
    /// - Only empty directories (or those containing only disposable OS files) are removed.
    pub fn clean_empty_ancestor_directories(
        prune_root: &Path,
        affected_file_paths: &[PathBuf],
        guard: &ArchiveProtectionGuard,
        execute: bool,
    ) -> (usize, Vec<String>) {
        let norm_root = ArchiveProtectionGuard::canonicalize_safe(prune_root);
        let mut removed_count = 0usize;
        let mut errors = Vec::new();

        if execute {
            // Collect all unique parent directories of deleted files
            let mut candidate_dirs: HashSet<PathBuf> = HashSet::new();
            for file_path in affected_file_paths {
                if let Some(parent) = file_path.parent() {
                    candidate_dirs.insert(parent.to_path_buf());
                }
            }

            // Sort candidates by component count descending (deepest paths first)
            let mut sorted_dirs: Vec<PathBuf> = candidate_dirs.into_iter().collect();
            sorted_dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));

            for dir in sorted_dirs {
                let mut current = dir;
                while let Some(parent) = current.parent() {
                    let norm_current = ArchiveProtectionGuard::canonicalize_safe(&current);
                    // Boundary Guard 1: Never delete target prune root or outside it
                    if !norm_current.starts_with(&norm_root) || norm_current == norm_root {
                        break;
                    }
                    // Boundary Guard 2: Never delete protected master archives
                    if guard.is_protected_path(&current) {
                        break;
                    }
                    if !current.exists() {
                        current = parent.to_path_buf();
                        continue;
                    }

                    match check_directory_empty(&current, true) {
                        Ok(true) => match std::fs::remove_dir(&current) {
                            Ok(_) => {
                                removed_count += 1;
                                current = parent.to_path_buf();
                            }
                            Err(e) => {
                                tracing::debug!(
                                    dir = %current.display(),
                                    error = %e,
                                    "Directory not removed (may be non-empty or locked)"
                                );
                                break;
                            }
                        },
                        Ok(false) => {
                            // Directory contains other non-duplicate files, stop climbing this branch
                            break;
                        }
                        Err(e) => {
                            errors.push(format!(
                                "Failed reading directory '{}' during empty directory cleanup: {}",
                                current.display(),
                                e
                            ));
                            break;
                        }
                    }
                }
            }
        } else {
            // Dry-run simulation:
            // Files still exist physically on disk, so simulate their removal
            let mut simulated_deleted: HashSet<PathBuf> = affected_file_paths
                .iter()
                .map(|p| ArchiveProtectionGuard::canonicalize_safe(p))
                .collect();

            let mut candidate_dirs: HashSet<PathBuf> = HashSet::new();
            for file_path in affected_file_paths {
                if let Some(parent) = file_path.parent() {
                    candidate_dirs.insert(parent.to_path_buf());
                }
            }

            let mut sorted_dirs: Vec<PathBuf> = candidate_dirs.into_iter().collect();
            sorted_dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));

            for dir in sorted_dirs {
                let mut current = dir;
                while let Some(parent) = current.parent() {
                    let norm_current = ArchiveProtectionGuard::canonicalize_safe(&current);
                    if !norm_current.starts_with(&norm_root) || norm_current == norm_root {
                        break;
                    }
                    if guard.is_protected_path(&current) {
                        break;
                    }
                    if !current.exists() {
                        current = parent.to_path_buf();
                        continue;
                    }

                    // Check if all entries in current directory would be removed
                    let entries_res = std::fs::read_dir(&current);
                    let mut would_be_empty = true;
                    if let Ok(entries) = entries_res {
                        for entry_res in entries {
                            if let Ok(entry) = entry_res {
                                let file_name = entry.file_name();
                                let name_str = file_name.to_string_lossy();
                                if is_disposable_os_file(&name_str) {
                                    continue;
                                }
                                let norm_entry =
                                    ArchiveProtectionGuard::canonicalize_safe(&entry.path());
                                if !simulated_deleted.contains(&norm_entry) {
                                    would_be_empty = false;
                                    break;
                                }
                            } else {
                                would_be_empty = false;
                                break;
                            }
                        }
                    } else {
                        would_be_empty = false;
                    }

                    if would_be_empty {
                        removed_count += 1;
                        simulated_deleted.insert(norm_current);
                        current = parent.to_path_buf();
                    } else {
                        break;
                    }
                }
            }
        }

        (removed_count, errors)
    }
}
