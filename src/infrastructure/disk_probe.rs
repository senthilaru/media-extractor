use std::collections::HashMap;
use std::ffi::CString;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

/// Abstraction for querying available disk space on mount paths.
pub trait DiskSpaceProbe: Send + Sync + std::fmt::Debug {
    /// Returns the available space in bytes for unprivileged users at the given path.
    fn available_space_bytes(&self, path: &Path) -> io::Result<u64>;
}

/// POSIX implementation of `DiskSpaceProbe` using `libc::statvfs`.
#[derive(Debug, Default, Clone)]
pub struct PosixDiskSpaceProbe;

impl PosixDiskSpaceProbe {
    pub fn new() -> Self {
        Self
    }
}

impl DiskSpaceProbe for PosixDiskSpaceProbe {
    fn available_space_bytes(&self, path: &Path) -> io::Result<u64> {
        // Find existing parent if the exact path does not exist yet
        let mut probe_path = path.to_path_buf();
        while !probe_path.exists() {
            if let Some(parent) = probe_path.parent() {
                probe_path = parent.to_path_buf();
            } else {
                break;
            }
        }

        let c_path = CString::new(probe_path.to_string_lossy().as_bytes())
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;

        #[cfg(unix)]
        unsafe {
            let mut stat: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c_path.as_ptr(), &mut stat) != 0 {
                return Err(io::Error::last_os_error());
            }
            // f_bavail is free blocks for unprivileged users, f_frsize is fragment size
            let available = (stat.f_bavail as u64).saturating_mul(stat.f_frsize as u64);
            Ok(available)
        }

        #[cfg(not(unix))]
        {
            // Fallback for non-unix testing environments: return effectively unlimited space
            Ok(u64::MAX)
        }
    }
}

/// In-memory mock disk space probe for testing volume spillover.
#[derive(Debug, Clone, Default)]
pub struct MockDiskSpaceProbe {
    mount_spaces: Arc<RwLock<HashMap<PathBuf, u64>>>,
    default_space: Arc<RwLock<u64>>,
}

impl MockDiskSpaceProbe {
    pub fn new() -> Self {
        Self {
            mount_spaces: Arc::new(RwLock::new(HashMap::new())),
            default_space: Arc::new(RwLock::new(u64::MAX)),
        }
    }

    pub fn with_default_space(default_space: u64) -> Self {
        Self {
            mount_spaces: Arc::new(RwLock::new(HashMap::new())),
            default_space: Arc::new(RwLock::new(default_space)),
        }
    }

    pub fn set_available_space(&self, path: impl Into<PathBuf>, space_bytes: u64) {
        let mut map = self.mount_spaces.write().unwrap();
        map.insert(path.into(), space_bytes);
    }
}

impl DiskSpaceProbe for MockDiskSpaceProbe {
    fn available_space_bytes(&self, path: &Path) -> io::Result<u64> {
        let map = self.mount_spaces.read().unwrap();
        // Check exact match or prefixes
        if let Some(&space) = map.get(path) {
            return Ok(space);
        }

        for (mount, &space) in map.iter() {
            if path.starts_with(mount) {
                return Ok(space);
            }
        }

        let default_space = *self.default_space.read().unwrap();
        Ok(default_space)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_disk_space_probe() {
        let probe = MockDiskSpaceProbe::new();
        probe.set_available_space("/mnt/vol1", 1024 * 1024);
        probe.set_available_space("/mnt/vol2", 50 * 1024 * 1024);

        assert_eq!(
            probe.available_space_bytes(Path::new("/mnt/vol1/media")).unwrap(),
            1024 * 1024
        );
        assert_eq!(
            probe.available_space_bytes(Path::new("/mnt/vol2/data/file.jpg")).unwrap(),
            50 * 1024 * 1024
        );
        assert_eq!(
            probe.available_space_bytes(Path::new("/mnt/unknown")).unwrap(),
            u64::MAX
        );
    }

    #[test]
    fn test_posix_disk_probe_current_dir() {
        let probe = PosixDiskSpaceProbe::new();
        let space = probe.available_space_bytes(Path::new(".")).unwrap();
        assert!(space > 0);
    }
}
