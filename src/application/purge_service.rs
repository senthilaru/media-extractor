use std::collections::BTreeSet;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use chrono::Utc;
use tracing::{info, warn};

use crate::domain::dedup_models::DriveConfiguration;
use crate::domain::errors::DeduplicationError;
use crate::domain::ports::MetadataStorageSink;
use crate::infrastructure::archive_guard::ArchiveProtectionGuard;

/// Options configuring the purge operation.
#[derive(Debug, Clone)]
pub struct PurgeOptions {
    /// Whether to actually delete matched documents from MongoDB (false = dry run preview)
    pub execute: bool,
    /// Whether to also delete physical source files under source_path from local disk
    pub delete_source: bool,
    /// Destination path for the manual deletion manifest file
    pub manifest_output_path: PathBuf,
}

impl Default for PurgeOptions {
    fn default() -> Self {
        Self {
            execute: false,
            delete_source: false,
            manifest_output_path: PathBuf::from("manual_deletion_paths.txt"),
        }
    }
}

/// Discovered file entry for manual deletion report.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DiscoveredPurgeFile {
    pub file_path: PathBuf,
    pub exists_on_disk: bool,
    pub file_size_bytes: u64,
}

/// Comprehensive summary report of a source purge run.
#[derive(Debug, Clone, serde::Serialize)]
pub struct PurgeReport {
    pub source_prefix: String,
    pub total_matched_records: usize,
    pub records_purged_from_db: usize,
    pub primary_master_files: Vec<PathBuf>,
    pub replica_files: Vec<PathBuf>,
    pub source_files: Vec<PathBuf>,
    pub source_files_deleted: usize,
    pub manifest_file_path: PathBuf,
    pub is_dry_run: bool,
}

/// Service that purges all traces of an unwanted source folder from the media catalog.
///
/// It queries all database documents originating from the source path,
/// deletes them from MongoDB (when --execute is specified),
/// and outputs all canonical master and replica filepaths to a dedicated manifest file
/// so the user can review and manually delete them (preventing accidental automated master loss).
pub struct PurgeService {
    sink: Arc<dyn MetadataStorageSink>,
    guard: ArchiveProtectionGuard,
    config: DriveConfiguration,
}

struct PurgeManifestData<'a> {
    output_path: &'a Path,
    source_prefix: &'a str,
    total_matched: usize,
    master_files: &'a [PathBuf],
    replica_files: &'a [PathBuf],
    source_files: &'a [PathBuf],
    executed: bool,
}

impl PurgeService {
    pub fn new(sink: Arc<dyn MetadataStorageSink>, config: DriveConfiguration) -> Self {
        let guard = ArchiveProtectionGuard::from_config(&config);
        Self {
            sink,
            guard,
            config,
        }
    }

    /// Purges all media records originating from `source_path`.
    pub async fn purge_source(
        &self,
        source_path: &Path,
        options: &PurgeOptions,
    ) -> Result<PurgeReport, DeduplicationError> {
        let source_str = source_path.to_string_lossy().trim_end_matches('/').to_string();
        info!(
            source_path = %source_str,
            execute = options.execute,
            delete_source = options.delete_source,
            "Initiating source purge operation"
        );

        // 1. Query all documents originating from or referencing the source path
        let docs = self
            .sink
            .query_by_source_path_prefix(&source_str)
            .await
            .map_err(DeduplicationError::Storage)?;

        let total_matched = docs.len();

        // 2. Identify drive boundaries
        let primary_master_root = self
            .config
            .drives
            .iter()
            .find(|d| d.group == "primary")
            .map(|d| ArchiveProtectionGuard::canonicalize_safe(&d.master_path));

        let replica_roots: Vec<PathBuf> = self
            .config
            .drives
            .iter()
            .filter(|d| d.group.starts_with("replica"))
            .map(|d| ArchiveProtectionGuard::canonicalize_safe(&d.master_path))
            .collect();

        let norm_source_prefix = ArchiveProtectionGuard::canonicalize_safe(source_path);

        let mut master_paths: BTreeSet<PathBuf> = BTreeSet::new();
        let mut replica_paths: BTreeSet<PathBuf> = BTreeSet::new();
        let mut source_paths: BTreeSet<PathBuf> = BTreeSet::new();

        for doc in &docs {
            let mut paths_to_check: Vec<PathBuf> = vec![doc.file_context.file_path.clone()];

            if let Some(ref dedup) = doc.dedup {
                if let Some(ref master_p) = dedup.canonical_master_path {
                    paths_to_check.push(master_p.clone());
                }
            }

            if let Some(ref fp) = doc.fingerprint {
                paths_to_check.push(fp.file_path.clone());
            }

            for p in paths_to_check {
                let norm_p = ArchiveProtectionGuard::canonicalize_safe(&p);

                if let Some(ref pm_root) = primary_master_root {
                    if norm_p.starts_with(pm_root) {
                        master_paths.insert(p);
                        continue;
                    }
                }

                let mut is_replica = false;
                for rep_root in &replica_roots {
                    if norm_p.starts_with(rep_root) {
                        replica_paths.insert(p.clone());
                        is_replica = true;
                        break;
                    }
                }
                if is_replica {
                    continue;
                }

                if norm_p.starts_with(&norm_source_prefix)
                    || p.to_string_lossy().starts_with(&source_str)
                {
                    source_paths.insert(p);
                }
            }
        }

        let primary_master_vec: Vec<PathBuf> = master_paths.into_iter().collect();
        let replica_vec: Vec<PathBuf> = replica_paths.into_iter().collect();
        let source_vec: Vec<PathBuf> = source_paths.into_iter().collect();

        // 3. Generate the manual deletion manifest file
        let manifest_data = PurgeManifestData {
            output_path: &options.manifest_output_path,
            source_prefix: &source_str,
            total_matched,
            master_files: &primary_master_vec,
            replica_files: &replica_vec,
            source_files: &source_vec,
            executed: options.execute,
        };
        Self::write_manifest(&manifest_data)?;

        // 4. Optionally delete source files on disk
        let mut source_files_deleted = 0usize;
        if options.delete_source {
            for p in &source_vec {
                if p.exists() {
                    match self.guard.safe_delete_file(p) {
                        Ok(_) => {
                            source_files_deleted += 1;
                        }
                        Err(e) => {
                            warn!(path = %p.display(), error = %e, "Failed to delete source file");
                        }
                    }
                }
            }
        }

        // 5. Purge documents from MongoDB if execute is true
        let mut records_purged = 0usize;
        if options.execute && !docs.is_empty() {
            let doc_ids: Vec<uuid::Uuid> = docs.iter().map(|d| d.id).collect();
            records_purged = self
                .sink
                .purge_documents_by_ids(&doc_ids)
                .await
                .map_err(DeduplicationError::Storage)?;
            info!(
                records_purged = records_purged,
                "Successfully purged documents from MongoDB"
            );
        }

        Ok(PurgeReport {
            source_prefix: source_str,
            total_matched_records: total_matched,
            records_purged_from_db: records_purged,
            primary_master_files: primary_master_vec,
            replica_files: replica_vec,
            source_files: source_vec,
            source_files_deleted,
            manifest_file_path: options.manifest_output_path.clone(),
            is_dry_run: !options.execute,
        })
    }

    fn write_manifest(data: &PurgeManifestData) -> Result<(), DeduplicationError> {
        let mut file =
            File::create(data.output_path).map_err(|e| DeduplicationError::PruneIoError {
                path: data.output_path.to_path_buf(),
                source: e,
            })?;

        writeln!(file, "# ==============================================================================").ok();
        writeln!(file, "# MANUAL MEDIA PURGE MANIFEST").ok();
        writeln!(file, "# ==============================================================================").ok();
        writeln!(file, "# Source Path Prefix   : {}", data.source_prefix).ok();
        writeln!(file, "# Generated At (UTC)   : {}", Utc::now().to_rfc3339()).ok();
        writeln!(file, "# Matched Catalog Docs : {}", data.total_matched).ok();
        writeln!(
            file,
            "# Database Status      : {}",
            if data.executed {
                "PURGED FROM MONGODB"
            } else {
                "PREVIEW (Dry-Run: DB records preserved)"
            }
        )
        .ok();
        writeln!(file, "#").ok();
        writeln!(file, "# CRITICAL SAFETY NOTICE:").ok();
        writeln!(file, "# Master and replica archives are strictly protected against automated programmatic").ok();
        writeln!(file, "# deletion to prevent irreversible loss. Below are the on-disk file paths found").ok();
        writeln!(file, "# across your primary master and replica drives. Review them carefully and delete").ok();
        writeln!(file, "# them manually using the provided shell commands if desired.").ok();
        writeln!(file, "# ==============================================================================").ok();
        writeln!(file).ok();

        // Section 1: Primary Master Files
        writeln!(file, "### PRIMARY MASTER FILES ({})", data.master_files.len()).ok();
        if data.master_files.is_empty() {
            writeln!(file, "# None found in primary master archive.").ok();
        } else {
            for p in data.master_files {
                let (exists_str, size_str) = get_file_info(p);
                writeln!(file, "{} ({}) [{}]", p.display(), size_str, exists_str).ok();
            }
        }
        writeln!(file).ok();

        // Section 2: Replica Files
        writeln!(file, "### REPLICA FILES ({})", data.replica_files.len()).ok();
        if data.replica_files.is_empty() {
            writeln!(file, "# None found on replica drives.").ok();
        } else {
            for p in data.replica_files {
                let (exists_str, size_str) = get_file_info(p);
                writeln!(file, "{} ({}) [{}]", p.display(), size_str, exists_str).ok();
            }
        }
        writeln!(file).ok();

        // Section 3: Source Files
        writeln!(file, "### SOURCE FILES ({})", data.source_files.len()).ok();
        if data.source_files.is_empty() {
            writeln!(file, "# None found at source location.").ok();
        } else {
            for p in data.source_files {
                let (exists_str, size_str) = get_file_info(p);
                writeln!(file, "{} ({}) [{}]", p.display(), size_str, exists_str).ok();
            }
        }
        writeln!(file).ok();

        // Section 4: Shell Script Helper
        writeln!(file, "# ==============================================================================").ok();
        writeln!(file, "# SHELL COMMANDS FOR MANUAL REMOVAL").ok();
        writeln!(file, "# ==============================================================================").ok();
        writeln!(file, "# To delete the primary master and replica files manually from disk, copy and run:").ok();
        writeln!(file).ok();

        let all_archive_files: Vec<&PathBuf> = data
            .master_files
            .iter()
            .chain(data.replica_files.iter())
            .collect();
        if all_archive_files.is_empty() {
            writeln!(file, "# No master or replica files to delete.").ok();
        } else {
            writeln!(file, "rm -f \\").ok();
            for (idx, p) in all_archive_files.iter().enumerate() {
                let is_last = idx == all_archive_files.len() - 1;
                let sep = if is_last { "" } else { " \\" };
                writeln!(file, "  '{}'{}", p.display(), sep).ok();
            }
        }
        writeln!(file).ok();

        Ok(())
    }
}

fn get_file_info(p: &Path) -> (&'static str, String) {
    if p.exists() {
        if let Ok(meta) = std::fs::metadata(p) {
            let bytes = meta.len();
            let formatted = format_bytes(bytes);
            ("EXISTS", formatted)
        } else {
            ("EXISTS", "unknown size".to_string())
        }
    } else {
        ("DOES NOT EXIST", "0 B".to_string())
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
