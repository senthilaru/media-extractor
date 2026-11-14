use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedMutexGuard};

/// RAII guard holding spindle mutex locks for drives involved in a transfer.
/// When dropped, all acquired spindle locks are automatically released.
pub struct SpindleTransferGuard {
    _source_guard: Option<OwnedMutexGuard<()>>,
    _target_guard: Option<OwnedMutexGuard<()>>,
}

/// Coordinates physical spindle serialization to prevent concurrent head-seeking contention
/// while allowing inter-drive concurrency across independent physical drives.
#[derive(Debug, Default, Clone)]
pub struct DriveSpindleLockManager {
    mounts: Arc<Mutex<Vec<PathBuf>>>,
    locks: Arc<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>>,
}

impl DriveSpindleLockManager {
    pub fn new() -> Self {
        Self {
            mounts: Arc::new(Mutex::new(Vec::new())),
            locks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Registers a known drive mount path (e.g. from drive configuration).
    pub async fn register_mount_point(&self, mount: impl Into<PathBuf>) {
        let p = mount.into();
        let mut mounts = self.mounts.lock().await;
        if !mounts.contains(&p) {
            mounts.push(p);
        }
    }

    /// Resolves the physical spindle root for a given file path.
    pub async fn resolve_spindle_root(&self, path: &Path) -> PathBuf {
        let mounts = self.mounts.lock().await;
        for m in mounts.iter() {
            if path.starts_with(m) {
                return m.clone();
            }
        }
        drop(mounts);

        Self::detect_spindle_root(path)
    }

    /// Detects volume or mount root based on OS path conventions or filesystem root.
    pub fn detect_spindle_root(path: &Path) -> PathBuf {
        let components: Vec<_> = path.components().collect();
        if components.len() >= 3 {
            use std::path::Component;
            if let (Component::RootDir, Component::Normal(c1), Component::Normal(c2)) =
                (components[0], components[1], components[2])
            {
                let s1 = c1.to_string_lossy();
                let s2 = c2.to_string_lossy();
                if s1 == "Volumes" || s1 == "mnt" {
                    return PathBuf::from("/").join(&*s1).join(&*s2);
                }
                if s1 == "media" && components.len() >= 4 {
                    if let Component::Normal(c3) = components[3] {
                        let s3 = c3.to_string_lossy();
                        return PathBuf::from("/").join(&*s1).join(&*s2).join(&*s3);
                    }
                }
            }
        }

        // Fallback: directory parent if path has extension/file name, or path itself
        if path.is_file() || path.extension().is_some() {
            path.parent().unwrap_or(path).to_path_buf()
        } else {
            path.to_path_buf()
        }
    }

    /// Gets or creates the Arc<Mutex<()>> for a spindle root.
    async fn get_lock_for_root(&self, root: &Path) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().await;
        locks
            .entry(root.to_path_buf())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    /// Acquires an exclusive spindle lock for a single path.
    pub async fn acquire_spindle_lock(&self, path: &Path) -> OwnedMutexGuard<()> {
        let root = self.resolve_spindle_root(path).await;
        let lock = self.get_lock_for_root(&root).await;
        lock.lock_owned().await
    }

    /// Acquires exclusive spindle lock(s) for a transfer from source to target.
    /// If both reside on the same physical drive, only a single lock is acquired.
    /// If they reside on different physical drives, locks are acquired in canonical sorted order
    /// to prevent deadlocks.
    pub async fn acquire_transfer_locks(
        &self,
        source: &Path,
        target: &Path,
    ) -> SpindleTransferGuard {
        let src_root = self.resolve_spindle_root(source).await;
        let tgt_root = self.resolve_spindle_root(target).await;

        if src_root == tgt_root {
            let lock = self.get_lock_for_root(&src_root).await;
            let guard = lock.lock_owned().await;
            SpindleTransferGuard {
                _source_guard: Some(guard),
                _target_guard: None,
            }
        } else if src_root < tgt_root {
            let src_lock = self.get_lock_for_root(&src_root).await;
            let src_guard = src_lock.lock_owned().await;
            let tgt_lock = self.get_lock_for_root(&tgt_root).await;
            let tgt_guard = tgt_lock.lock_owned().await;
            SpindleTransferGuard {
                _source_guard: Some(src_guard),
                _target_guard: Some(tgt_guard),
            }
        } else {
            let tgt_lock = self.get_lock_for_root(&tgt_root).await;
            let tgt_guard = tgt_lock.lock_owned().await;
            let src_lock = self.get_lock_for_root(&src_root).await;
            let src_guard = src_lock.lock_owned().await;
            SpindleTransferGuard {
                _source_guard: Some(src_guard),
                _target_guard: Some(tgt_guard),
            }
        }
    }
}
