# Data Model: Scan Progress Telemetry & Performance Optimization

**Feature**: `007-scan-progress-performance`  
**Date**: 2026-11-08  
**Status**: Completed  

---

## 1. Domain Entities & Value Objects

### 1.1 `OperationalPhase`

Enumeration representing the active high-level stage of pipeline execution.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum OperationalPhase {
    /// Walking filesystem directory tree and cataloging media items
    Discovery,
    /// Extracting container metadata, EXIF, QuickTime, and dimensions
    Extraction,
    /// Computing BLAKE3 cryptographic and perceptual DCT visual hashes
    Fingerprinting,
    /// Ingesting documents to MongoDB storage sink or writing output
    Persistence,
    /// Pipeline completed, queues drained, resources released
    Completed,
}
```

### 1.2 `ExtractionMode`

High-level performance profile controlling extraction depth.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum, serde::Serialize, serde::Deserialize)]
pub enum ExtractionMode {
    /// Fast metadata-only mode: container headers, EXIF/QuickTime, dimensions, timestamps.
    /// Bypasses full-file bitwise cryptographic hashing and perceptual visual hashing.
    Fast,
    /// Balanced mode: Fast metadata + sample hashing; deep hashing only for files below threshold.
    Balanced,
    /// Full mode: Complete bitwise cryptographic hashing and perceptual deduplication for all files.
    Full,
}
```

### 1.3 `ExtractionPerformanceProfile`

Encapsulates user-configured or profile-derived runtime performance options.

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtractionPerformanceProfile {
    /// High-level mode (Fast, Balanced, Full)
    pub mode: ExtractionMode,
    /// File size limit in bytes above which full-file cryptographic hashing is skipped (e.g. 100 MB).
    /// None means no size limit (hash regardless of size).
    pub max_hash_size_bytes: Option<u64>,
    /// Whether perceptual visual hashing (pHash) is active
    pub enable_perceptual: bool,
    /// Whether full cryptographic bitwise hashing (BLAKE3) is active
    pub enable_cryptographic_hash: bool,
    /// Dedicated extraction worker pool concurrency (defaults to host logical cores)
    pub worker_concurrency: usize,
    /// Batch size for discovered items
    pub batch_size: usize,
    /// Channel capacity for bounded memory backpressure
    pub channel_capacity: usize,
    /// Cadence for progress updates in milliseconds (default: 250ms = 4 fps)
    pub update_interval_ms: u64,
}
```

### 1.4 `ScanProgressSnapshot`

Point-in-time progress telemetry snapshot emitted periodically to standard error or structured consumers.

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScanProgressSnapshot {
    /// Active scan session identifier
    pub session_id: uuid::Uuid,
    /// Elapsed wall-clock duration since scan initiation
    pub elapsed_duration: std::time::Duration,
    /// Current operational phase
    pub active_phase: OperationalPhase,
    /// Directories visited during traversal
    pub directories_scanned: u64,
    /// Total media files discovered so far
    pub items_discovered: u64,
    /// Total byte volume discovered so far
    pub bytes_discovered: u64,
    /// Total media files extracted
    pub items_processed: u64,
    /// Total byte volume processed
    pub bytes_processed: u64,
    /// Items successfully persisted to database (if configured)
    pub items_persisted: u64,
    /// Instantaneous or moving-average processing throughput in items/second
    pub items_per_sec: f64,
    /// Instantaneous or moving-average data throughput in Megabytes/second
    pub mb_per_sec: f64,
    /// Estimated time remaining to complete processing
    pub estimated_remaining: Option<std::time::Duration>,
    /// Number of errors or skipped items encountered
    pub errors_encountered: u64,
    /// Whether directory discovery has finished enumerating all paths
    pub is_discovery_complete: bool,
}
```

### 1.5 `ExecutionSummary`

Post-scan audit report produced upon normal completion or signal interruption (Ctrl+C).

```rust
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExecutionSummary {
    /// Scan session identifier
    pub session_id: uuid::Uuid,
    /// Root path scanned
    pub target_path: std::path::PathBuf,
    /// Extraction performance mode utilized
    pub mode: ExtractionMode,
    /// Timestamp scan began
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Timestamp scan ended
    pub completed_at: chrono::DateTime<chrono::Utc>,
    /// Total elapsed wall-clock duration
    pub elapsed_duration: std::time::Duration,
    /// Total media files discovered
    pub total_discovered: u64,
    /// Total media files successfully extracted
    pub total_processed: u64,
    /// Total data volume processed in bytes
    pub total_bytes_processed: u64,
    /// Overall average throughput in items/second
    pub average_items_per_sec: f64,
    /// Overall average data rate in Megabytes/second
    pub average_mb_per_sec: f64,
    /// Database records upserted
    pub db_upserted: u64,
    /// Database records matched (existing identical records)
    pub db_matched: u64,
    /// Database records modified
    pub db_modified: u64,
    /// Database persistence errors
    pub db_failed: u64,
    /// Extraction or file read errors
    pub extraction_errors: u64,
    /// Extraction warnings (e.g. partial EXIF, corrupt tags)
    pub warnings_count: u64,
    /// Whether scan completed to 100% or was cancelled early
    pub was_cancelled: bool,
}
```

---

## 2. State Transitions & Lifecycle

```
[CLI Invoked] ──► [Discovery Phase]
                         │
                         ├──► [Bounded Channel] ──► [Concurrent Extraction Workers]
                         │                                    │
                         │                                    ├──► [Fast: Header/EXIF only]
                         │                                    └──► [Full: BLAKE3 mmap + pHash]
                         │                                                │
                         ▼                                                ▼
              [Live Progress Bar (stderr)] ◄──────────────────── [Persistence Sink]
                         │
                         ▼
             [Final Execution Summary] ──► [Exit 0]
```
