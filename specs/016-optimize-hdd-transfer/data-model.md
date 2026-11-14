# Data Model & Domain Entities: External HDD Transfer Optimization

**Feature**: `016-optimize-hdd-transfer`
**Date**: 2026-11-13
**Status**: Complete

## 1. Domain Entities & Value Objects

### 1.1 `TransferBufferConfig` (Value Object)
Encapsulates streaming buffer parameters and verification settings.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransferBufferConfig {
    /// Buffer chunk size in bytes (default: 8 MB)
    pub buffer_size_bytes: usize,
    /// Whether in-flight stream hashing is enabled (default: true)
    pub in_flight_hash: bool,
    /// Whether to force a secondary post-copy disk re-read hash check (default: false)
    pub recheck_disk_hash: bool,
}

impl Default for TransferBufferConfig {
    fn default() -> Self {
        Self {
            buffer_size_bytes: 8 * 1024 * 1024, // 8 MB optimal for external HDDs
            in_flight_hash: true,
            recheck_disk_hash: false,
        }
    }
}
```

#### Validation Rules:
- `buffer_size_bytes` MUST be $\ge 64 \text{ KB}$ (`65,536` bytes).
- `buffer_size_bytes` MUST be $\le 32 \text{ MB}$ (`33,554,432` bytes) to guarantee bounded memory.
- Can be parsed from human-readable strings like `"4M"`, `"8M"`, `"16M"`, `"4096K"`, or raw integer strings.

---

### 1.2 `InFlightStreamHasher` (Domain Helper)
Manages the cryptographic hashing state during contiguous stream copy.

```rust
pub struct InFlightStreamHasher {
    hasher: blake3::Hasher,
    expected_hash: Option<String>,
}

impl InFlightStreamHasher {
    pub fn new(expected_hash: Option<&str>) -> Self {
        Self {
            hasher: blake3::Hasher::new(),
            expected_hash: expected_hash.map(|s| s.to_string()),
        }
    }

    pub fn update(&mut self, chunk: &[u8]) {
        self.hasher.update(chunk);
    }

    pub fn finalize_and_verify(&self) -> Result<String, DeduplicationError> {
        let computed = self.hasher.finalize().to_hex().to_string();
        if let Some(ref expected) = self.expected_hash {
            if !expected.is_empty() && *expected != computed {
                return Err(DeduplicationError::TransferFailed {
                    source_path: Default::default(),
                    target_path: Default::default(),
                    reason: format!(
                        "In-flight hash mismatch: expected {}, computed {}",
                        expected, computed
                    ),
                });
            }
        }
        Ok(computed)
    }
}
```

---

### 1.3 `DriveSpindleLockManager` (Concurrency Entity)
Coordinates physical spindle serialization to prevent concurrent head-seeking contention on the same drive.

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Default, Clone)]
pub struct DriveSpindleLockManager {
    locks: Arc<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>>,
}
```

#### Behavior & State Transitions:
1. **Acquire**: Given a source or target file path, canonicalize its mount point (e.g. `/Volumes/SP-1`).
2. Retrieve or create the mutex associated with that physical mount point.
3. Lock the spindle mutex before commencing sequential file streaming.
4. **Release**: Drop the mutex guard immediately after `sync_all` and handle teardown, allowing the next queued file for that drive to stream.

---

### 1.4 Extended CLI Options
Updated `IngestArgs` and `SyncReplicasArgs`:

```rust
#[derive(clap::Args, Debug, Clone)]
pub struct IngestArgs {
    // ... existing fields ...
    
    /// I/O transfer buffer size for stream copies (e.g., '4M', '8M', '16M', default: '8M')
    #[arg(long, default_value = "8M")]
    pub buffer_size: String,

    /// Force a secondary disk re-read pass to verify destination hash (default: false, in-flight verify used)
    #[arg(long, default_value_t = false)]
    pub recheck_disk_hash: bool,
}
```
