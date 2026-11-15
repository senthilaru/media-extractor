use std::path::{Component, Path, PathBuf};
use tracing::{debug, error};

use crate::domain::dedup_models::{DriveConfiguration, DriveDefinition};
use crate::domain::errors::DeduplicationError;

/// Guard responsible for enforcing immutability across primary master and replica archives.
///
/// Prevents any pruning, deletion, or destructive operations from targeting primary
/// master archives or replica backup archives across all execution stages.
#[derive(Debug, Clone)]
pub struct ArchiveProtectionGuard {
    protected_roots: Vec<PathBuf>,
}

impl ArchiveProtectionGuard {
    /// Constructs a new protection guard with an explicit list of protected root directories.
    pub fn new(roots: Vec<PathBuf>) -> Self {
        let protected_roots = roots
            .into_iter()
            .map(|r| Self::canonicalize_safe(&r))
            .collect();
        Self { protected_roots }
    }

    /// Constructs a protection guard from a `DriveConfiguration`.
    ///
    /// Strictly protects the canonical master root directory of primary and replica drives.
    /// Files outside master_path on primary/replica drives (e.g. unorganized folders)
    /// are permissible for duplicate cleanup.
    pub fn from_config(config: &DriveConfiguration) -> Self {
        let mut roots = Vec::new();

        for drive in &config.drives {
            let is_primary = drive.group == "primary";
            let is_replica = drive.group.starts_with("replica");

            if is_primary || is_replica {
                roots.push(Self::canonicalize_safe(&drive.master_path));
            }
        }

        // Deduplicate roots
        roots.sort();
        roots.dedup();

        Self {
            protected_roots: roots,
        }
    }

    /// Returns a slice of all registered protected root directories.
    pub fn protected_roots(&self) -> &[PathBuf] {
        &self.protected_roots
    }

    /// Evaluates if a given filesystem path is located within any protected archive root.
    ///
    /// Canonicalizes the target path (resolving symbolic links and relative path segments)
    /// before performing prefix matching against protected roots.
    pub fn is_protected_path(&self, path: &Path) -> bool {
        let canonical_target = Self::canonicalize_safe(path);

        for root in &self.protected_roots {
            if canonical_target.starts_with(root) {
                return true;
            }
        }

        // If the path physically exists and is a symlink, verify both the symlink itself
        // and the symlink's resolved destination.
        if let Ok(dest) = std::fs::read_link(path) {
            let resolved_dest = if dest.is_relative() {
                if let Some(parent) = path.parent() {
                    parent.join(dest)
                } else {
                    dest
                }
            } else {
                dest
            };
            let canonical_dest = Self::canonicalize_safe(&resolved_dest);
            for root in &self.protected_roots {
                if canonical_dest.starts_with(root) {
                    return true;
                }
            }
        }

        false
    }

    /// Validates whether a target drive identifier or path is permissible for duplicate pruning.
    ///
    /// Fails immediately if:
    /// - The target path resolves to or inside any protected canonical master root directory.
    /// - The target matches a drive whose mount path directly equals its master path (entire drive is the master archive).
    pub fn validate_prune_target(
        &self,
        target_drive_or_path: &str,
        config: &DriveConfiguration,
    ) -> Result<(), DeduplicationError> {
        // 1. Check if target matches a defined drive by ID
        if let Some(drive) = config.drives.iter().find(|d| d.id == target_drive_or_path) {
            return Self::validate_drive_definition(drive);
        }

        let target_path = Path::new(target_drive_or_path);
        let canonical_target = Self::canonicalize_safe(target_path);

        // 2. Check if target matches a defined drive by mount_path or master_path
        for drive in &config.drives {
            let canonical_master = Self::canonicalize_safe(&drive.master_path);
            if canonical_target == canonical_master {
                return Err(DeduplicationError::PruneForbiddenOnProtectedArchive {
                    drive: drive.id.clone(),
                    path: target_path.to_path_buf(),
                    reason: format!(
                        "Pruning is strictly forbidden on canonical master archive directory '{}'.",
                        target_path.display()
                    ),
                });
            }

            let canonical_mount = Self::canonicalize_safe(&drive.mount_path);
            if canonical_target == canonical_mount {
                return Self::validate_drive_definition(drive);
            }
        }

        // 3. Check if target path falls within ANY protected master archive root
        if self.is_protected_path(target_path) {
            return Err(DeduplicationError::PruneForbiddenOnProtectedArchive {
                drive: target_drive_or_path.to_string(),
                path: target_path.to_path_buf(),
                reason: format!(
                    "Pruning is strictly forbidden on path '{}' because it resides within protected canonical master archive storage.",
                    target_path.display()
                ),
            });
        }

        Ok(())
    }

    /// Validates a single `DriveDefinition` against archive immutability rules.
    pub fn validate_drive_definition(drive: &DriveDefinition) -> Result<(), DeduplicationError> {
        let is_primary = drive.group == "primary";
        let is_replica = drive.group.starts_with("replica");

        if is_primary || is_replica {
            let canonical_mount = Self::canonicalize_safe(&drive.mount_path);
            let canonical_master = Self::canonicalize_safe(&drive.master_path);

            // If the drive's mount path is identical to or within its master path, the entire drive is the master archive
            if canonical_mount == canonical_master || canonical_mount.starts_with(&canonical_master)
            {
                return Err(DeduplicationError::PruneForbiddenOnProtectedArchive {
                    drive: drive.id.clone(),
                    path: drive.mount_path.clone(),
                    reason: format!(
                        "Pruning is strictly forbidden on drive '{}' because its mount path is configured directly as the master archive root.",
                        drive.id
                    ),
                });
            }
        }

        Ok(())
    }

    /// Safely deletes a file from the filesystem after verifying it does NOT reside
    /// in any protected Primary Master or Replica directory tree.
    ///
    /// Acts as an unbypassable runtime circuit breaker immediately before `std::fs::remove_file`.
    pub fn safe_delete_file(&self, path: &Path) -> Result<(), DeduplicationError> {
        if self.is_protected_path(path) {
            error!(
                path = %path.display(),
                "CRITICAL: Prevented deletion of file inside protected archive storage!"
            );
            return Err(DeduplicationError::PruneForbiddenOnProtectedArchive {
                drive: "protected_archive".to_string(),
                path: path.to_path_buf(),
                reason: format!(
                    "CRITICAL PROTECTION GUARD: Attempted deletion of file '{}' inside protected archive storage was blocked.",
                    path.display()
                ),
            });
        }

        debug!(path = %path.display(), "Executing safe deletion on prunable file");
        match std::fs::remove_file(path) {
            Ok(_) => Ok(()),
            Err(_e) => {
                // If deletion failed due to permission denial or locked attributes (e.g. macOS uchg / read-only),
                // attempt to unlock and retry deletion once.
                Self::attempt_unlock_file(path);
                std::fs::remove_file(path).map_err(|retry_err| DeduplicationError::PruneIoError {
                    path: path.to_path_buf(),
                    source: retry_err,
                })
            }
        }
    }

    /// Attempts to clear read-only permissions and platform-specific immutable / lock flags
    /// (such as macOS BSD uchg flag) on a file or directory that has already passed the archive protection guard.
    pub fn attempt_unlock_file(path: &Path) {
        // 1. Clear read-only permission if set
        if let Ok(metadata) = std::fs::metadata(path) {
            let mut perms = metadata.permissions();
            if perms.readonly() {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let mode = perms.mode() | 0o600;
                    perms.set_mode(mode);
                }
                #[cfg(not(unix))]
                {
                    #[allow(clippy::permissions_set_readonly_false)]
                    perms.set_readonly(false);
                }
                let _ = std::fs::set_permissions(path, perms);
            }
        }

        // 2. On macOS / Apple platforms, clear BSD user flags (such as uchg / UF_IMMUTABLE)
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::ffi::OsStrExt;
            if let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) {
                unsafe {
                    extern "C" {
                        fn chflags(
                            path: *const std::ffi::c_char,
                            flags: std::ffi::c_uint,
                        ) -> std::ffi::c_int;
                    }
                    chflags(c_path.as_ptr(), 0);
                }
            }
        }
    }

    /// Normalizes paths that contain spurious shell backslash escapes (e.g. `\ `)
    /// which occur when users wrap already-escaped shell paths in quotes.
    pub fn clean_escaped_path(path: &Path) -> PathBuf {
        if path.exists() {
            return path.to_path_buf();
        }
        let s = path.to_string_lossy();
        if s.contains('\\') {
            let unescaped = PathBuf::from(s.replace("\\ ", " ").replace("\\\\", "\\"));
            if unescaped.exists() {
                return unescaped;
            }
        }
        path.to_path_buf()
    }

    /// Safely canonicalizes a path by resolving symlinks and normalizing `.` and `..` segments.
    ///
    /// If the path exists on disk, `std::fs::canonicalize` is used.
    /// If the path does not exist, existing parent directories are canonicalized and
    /// the remaining relative segments are appended, falling back to lexical normalization.
    pub fn canonicalize_safe(path: &Path) -> PathBuf {
        let cleaned = Self::clean_escaped_path(path);
        let path = cleaned.as_path();
        if let Ok(canonical) = std::fs::canonicalize(path) {
            return canonical;
        }

        // Path might not exist yet; walk up to find the closest existing ancestor
        let mut ancestors_stack = Vec::new();
        let mut current = path;

        while !current.as_os_str().is_empty() {
            if let Ok(canonical_ancestor) = std::fs::canonicalize(current) {
                let mut resolved = canonical_ancestor;
                for part in ancestors_stack.into_iter().rev() {
                    resolved.push(part);
                }
                return resolved;
            }

            if let Some(file_name) = current.file_name() {
                ancestors_stack.push(file_name);
            }
            if let Some(parent) = current.parent() {
                current = parent;
            } else {
                break;
            }
        }

        // Fall back to lexical normalization
        Self::normalize_lexically(path)
    }

    /// Lexically normalizes path components without touching the filesystem.
    pub fn normalize_lexically(path: &Path) -> PathBuf {
        let mut out = PathBuf::new();
        for comp in path.components() {
            match comp {
                Component::CurDir => {}
                Component::ParentDir => {
                    out.pop();
                }
                c => out.push(c.as_os_str()),
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::dedup_models::{
        DriveConfiguration, DriveDefinition, DriveGroupSpec, DriveRules,
    };
    use std::collections::HashMap;
    use tempfile::tempdir;

    fn make_test_config(
        primary_dir: &Path,
        replica_dir: &Path,
        staging_dir: &Path,
    ) -> DriveConfiguration {
        let mut groups = HashMap::new();
        groups.insert(
            "primary".to_string(),
            DriveGroupSpec {
                priority: 1,
                description: Some("Master".to_string()),
            },
        );
        groups.insert(
            "replica_1".to_string(),
            DriveGroupSpec {
                priority: 2,
                description: Some("Backup 1".to_string()),
            },
        );
        groups.insert(
            "staging".to_string(),
            DriveGroupSpec {
                priority: 3,
                description: Some("Staging scratch".to_string()),
            },
        );

        let drives = vec![
            DriveDefinition {
                id: "primary_drive".to_string(),
                group: "primary".to_string(),
                mount_path: primary_dir.to_path_buf(),
                master_path: primary_dir.join("media/master"),
            },
            DriveDefinition {
                id: "replica_drive".to_string(),
                group: "replica_1".to_string(),
                mount_path: replica_dir.to_path_buf(),
                master_path: replica_dir.join("media/master"),
            },
            DriveDefinition {
                id: "staging_drive".to_string(),
                group: "staging".to_string(),
                mount_path: staging_dir.to_path_buf(),
                master_path: staging_dir.join("media/staging"),
            },
        ];

        DriveConfiguration {
            version: "1.0".to_string(),
            groups,
            drives,
            rules: DriveRules::default(),
        }
    }

    #[test]
    fn test_primary_and_replica_paths_are_protected() {
        let tmp = tempdir().unwrap();
        let primary = tmp.path().join("primary");
        let replica = tmp.path().join("replica");
        let staging = tmp.path().join("staging");

        std::fs::create_dir_all(&primary).unwrap();
        std::fs::create_dir_all(&replica).unwrap();
        std::fs::create_dir_all(&staging).unwrap();

        let config = make_test_config(&primary, &replica, &staging);
        let guard = ArchiveProtectionGuard::from_config(&config);

        let file_on_primary_master = primary.join("media/master/Videos/clip.mp4");
        let file_on_primary_unorganized = primary.join("unorganized/clip.mp4");
        let file_on_replica_master = replica.join("media/master/Photos/pic.jpg");
        let file_on_staging = staging.join("temp.mp4");

        assert!(guard.is_protected_path(&file_on_primary_master));
        assert!(guard.is_protected_path(&file_on_replica_master));
        // Files outside master_path on primary or staging are NOT protected
        assert!(!guard.is_protected_path(&file_on_primary_unorganized));
        assert!(!guard.is_protected_path(&file_on_staging));
    }

    #[test]
    fn test_validate_prune_target() {
        let tmp = tempdir().unwrap();
        let primary = tmp.path().join("primary");
        let replica = tmp.path().join("replica");
        let staging = tmp.path().join("staging");

        std::fs::create_dir_all(&primary).unwrap();
        std::fs::create_dir_all(&replica).unwrap();
        std::fs::create_dir_all(&staging).unwrap();

        let config = make_test_config(&primary, &replica, &staging);
        let guard = ArchiveProtectionGuard::from_config(&config);

        // Target by drive ID where drive has master_path != mount_path is allowed
        assert!(guard
            .validate_prune_target("primary_drive", &config)
            .is_ok());
        assert!(guard
            .validate_prune_target("replica_drive", &config)
            .is_ok());
        assert!(guard
            .validate_prune_target("staging_drive", &config)
            .is_ok());

        // Target master root directly is strictly forbidden
        assert!(guard
            .validate_prune_target(primary.join("media/master").to_str().unwrap(), &config)
            .is_err());
        assert!(guard
            .validate_prune_target(replica.join("media/master").to_str().unwrap(), &config)
            .is_err());

        // Target unorganized directory on primary is allowed
        let unorg_primary = primary.join("unorganized");
        std::fs::create_dir_all(&unorg_primary).unwrap();
        assert!(guard
            .validate_prune_target(unorg_primary.to_str().unwrap(), &config)
            .is_ok());
    }

    #[test]
    fn test_safe_delete_file_prevents_deletion_on_protected_path() {
        let tmp = tempdir().unwrap();
        let primary = tmp.path().join("primary");
        let replica = tmp.path().join("replica");
        let staging = tmp.path().join("staging");

        std::fs::create_dir_all(&primary).unwrap();
        std::fs::create_dir_all(&replica).unwrap();
        std::fs::create_dir_all(&staging).unwrap();

        let file_primary_master = primary.join("media/master/important.mp4");
        std::fs::create_dir_all(file_primary_master.parent().unwrap()).unwrap();
        std::fs::write(&file_primary_master, b"precious video").unwrap();

        let file_primary_unorganized = primary.join("unorganized/temp.mp4");
        std::fs::create_dir_all(file_primary_unorganized.parent().unwrap()).unwrap();
        std::fs::write(&file_primary_unorganized, b"unorganized video").unwrap();

        let file_staging = staging.join("scratch.mp4");
        std::fs::write(&file_staging, b"scratch video").unwrap();

        let config = make_test_config(&primary, &replica, &staging);
        let guard = ArchiveProtectionGuard::from_config(&config);

        // Attempt deletion of primary master file
        let err = guard.safe_delete_file(&file_primary_master).unwrap_err();
        assert!(matches!(
            err,
            DeduplicationError::PruneForbiddenOnProtectedArchive { .. }
        ));
        assert!(
            file_primary_master.exists(),
            "Primary master file must NEVER be deleted"
        );

        // Safe deletion of unorganized file on primary succeeds
        assert!(guard.safe_delete_file(&file_primary_unorganized).is_ok());
        assert!(
            !file_primary_unorganized.exists(),
            "Unorganized file on primary should be deleted"
        );

        // Safe deletion of staging file succeeds
        assert!(guard.safe_delete_file(&file_staging).is_ok());
        assert!(!file_staging.exists(), "Staging file should be deleted");
    }

    #[test]
    fn test_symlink_pointing_to_primary_is_protected() {
        let tmp = tempdir().unwrap();
        let primary = tmp.path().join("primary");
        let replica = tmp.path().join("replica");
        let staging = tmp.path().join("staging");

        std::fs::create_dir_all(&primary).unwrap();
        std::fs::create_dir_all(&replica).unwrap();
        std::fs::create_dir_all(&staging).unwrap();

        let file_primary_master = primary.join("media/master/target.mp4");
        std::fs::create_dir_all(file_primary_master.parent().unwrap()).unwrap();
        std::fs::write(&file_primary_master, b"target media").unwrap();

        // Symlink in staging folder pointing to primary master file
        let symlink_path = staging.join("symlink_to_primary.mp4");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&file_primary_master, &symlink_path).unwrap();

        #[cfg(unix)]
        {
            let config = make_test_config(&primary, &replica, &staging);
            let guard = ArchiveProtectionGuard::from_config(&config);

            assert!(guard.is_protected_path(&symlink_path));
            assert!(guard.safe_delete_file(&symlink_path).is_err());
            assert!(file_primary_master.exists());
        }
    }

    #[test]
    fn test_safe_delete_locked_uchg_or_readonly_file() {
        let tmp = tempdir().unwrap();
        let primary = tmp.path().join("primary");
        let replica = tmp.path().join("replica");
        let staging = tmp.path().join("staging");

        std::fs::create_dir_all(&primary).unwrap();
        std::fs::create_dir_all(&replica).unwrap();
        std::fs::create_dir_all(&staging).unwrap();

        let locked_file = staging.join("locked_media.jpg");
        std::fs::write(&locked_file, b"read only or uchg content").unwrap();

        // Make file read-only
        let mut perms = std::fs::metadata(&locked_file).unwrap().permissions();
        perms.set_readonly(true);
        std::fs::set_permissions(&locked_file, perms).unwrap();

        // On macOS, also set uchg flag
        #[cfg(target_os = "macos")]
        {
            use std::os::unix::ffi::OsStrExt;
            let c_path = std::ffi::CString::new(locked_file.as_os_str().as_bytes()).unwrap();
            unsafe {
                extern "C" {
                    fn chflags(
                        path: *const std::ffi::c_char,
                        flags: std::ffi::c_uint,
                    ) -> std::ffi::c_int;
                }
                // UF_IMMUTABLE (uchg) = 0x0002
                let _ = chflags(c_path.as_ptr(), 0x0002);
            }
        }

        let config = make_test_config(&primary, &replica, &staging);
        let guard = ArchiveProtectionGuard::from_config(&config);

        // Safe delete should unlock and delete the file successfully
        let res = guard.safe_delete_file(&locked_file);
        assert!(
            res.is_ok(),
            "Expected safe_delete_file to succeed on locked file, got {:?}",
            res
        );
        assert!(!locked_file.exists(), "Locked file should have been deleted");
    }

    #[test]
    fn test_clean_escaped_path() {
        let tmp = tempdir().unwrap();
        let target_dir = tmp.path().join("Folder With Spaces/Sub Folder");
        std::fs::create_dir_all(&target_dir).unwrap();

        // Simulate path with literal escaped spaces "\ " as produced when user wraps escaped path in quotes
        let path_with_escapes = PathBuf::from(format!(
            "{}/Folder\\ With\\ Spaces/Sub\\ Folder",
            tmp.path().display()
        ));

        assert!(!path_with_escapes.exists());
        let cleaned = ArchiveProtectionGuard::clean_escaped_path(&path_with_escapes);
        assert!(cleaned.exists());
        assert_eq!(cleaned, target_dir);
    }
}
