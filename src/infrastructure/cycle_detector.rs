use std::collections::HashSet;
use std::path::Path;

/// Detects circular directory links and visits by tracking filesystem device and inode IDs.
#[derive(Debug, Default)]
pub struct CycleDetector {
    visited_inodes: HashSet<(u64, u64)>,
}

impl CycleDetector {
    pub fn new() -> Self {
        Self {
            visited_inodes: HashSet::new(),
        }
    }

    /// Checks if a directory target has already been visited.
    /// If not visited, records the target and returns `false`.
    /// If already visited, returns `true` (cycle detected).
    #[cfg(unix)]
    pub fn is_or_record_visited(&mut self, path: &Path) -> Result<bool, std::io::Error> {
        use std::os::unix::fs::MetadataExt;
        let meta = std::fs::metadata(path)?;
        let key = (meta.dev(), meta.ino());
        if !self.visited_inodes.insert(key) {
            Ok(true) // already present -> loop detected
        } else {
            Ok(false)
        }
    }

    #[cfg(not(unix))]
    pub fn is_or_record_visited(&mut self, path: &Path) -> Result<bool, std::io::Error> {
        // Fallback for non-unix platforms
        let canonical = std::fs::canonicalize(path)?;
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        canonical.hash(&mut hasher);
        let key = (0, hasher.finish());
        if !self.visited_inodes.insert(key) {
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
