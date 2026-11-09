# Contract: Scanner Public API & Traits

**Feature**: `001-media-directory-walker`  
**Date**: 2026-11-07  
**Status**: Completed  

---

## 1. Domain Ports (Traits)

Following Hexagonal Architecture and SOLID principles, the core scanning domain defines explicit ports implemented by infrastructure adapters.

```rust
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::mpsc;
use crate::domain::models::{DiscoveryBatch, ScanErrorRecord, ScanSession, SessionMetrics};
use crate::domain::errors::ScanError;

/// Configuration options for a directory scan run.
#[derive(Debug, Clone)]
pub struct ScanConfig {
    /// Root paths to traverse.
    pub root_paths: Vec<PathBuf>,
    /// Number of media items grouped per batch (default: 500).
    pub batch_size: usize,
    /// Bounded capacity for the batch channel (default: 100 batches).
    pub channel_capacity: usize,
    /// Whether to skip hidden files and directories (default: true).
    pub skip_hidden: bool,
    /// Whether to follow directory symlinks (default: false).
    pub follow_symlinks: bool,
    /// Interval for emitting telemetry heartbeats (default: 1000ms).
    pub telemetry_interval: Duration,
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            root_paths: Vec::new(),
            batch_size: 500,
            channel_capacity: 100,
            skip_hidden: true,
            follow_symlinks: false,
            telemetry_interval: Duration::from_millis(1000),
        }
    }
}

/// Output streams produced by an active scan execution.
pub struct ScanStream {
    /// Channel delivering bounded batches of discovered media items.
    pub batches: mpsc::Receiver<DiscoveryBatch>,
    /// Channel delivering non-fatal error records (permission denied, cycles).
    pub errors: mpsc::Receiver<ScanErrorRecord>,
    /// Telemetry channel emitting periodic performance snapshots.
    pub telemetry: mpsc::Receiver<SessionMetrics>,
}

/// Core port for directory traversal engines.
#[async_trait::async_trait]
pub trait DirectoryScanner: Send + Sync {
    /// Start an asynchronous directory scan session according to the provided configuration.
    /// Returns the scan session aggregate and the streaming channels.
    async fn start_scan(
        &self,
        config: ScanConfig,
    ) -> Result<(ScanSession, ScanStream), ScanError>;

    /// Request cancellation of an in-progress scan session.
    async fn cancel_scan(&self, session_id: &uuid::Uuid) -> Result<(), ScanError>;
}
```

---

## 2. Media Classifier Port

```rust
use std::path::Path;
use crate::domain::models::MediaCategory;

/// Port for classifying file paths into media categories.
pub trait MediaClassifier: Send + Sync {
    /// Classifies a path based on its extension or characteristics without disk reads.
    fn classify(&self, path: &Path) -> Option<MediaCategory>;

    /// Returns whether the provided extension is a supported media format.
    fn is_supported_extension(&self, extension: &str) -> bool;
}
```
