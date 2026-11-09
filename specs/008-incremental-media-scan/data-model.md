# Data Model: Incremental Media Scan & Database Change Caching

**Feature**: `008-incremental-media-scan`  
**Date**: 2026-11-09  
**Status**: Completed  

---

## 1. Domain Entities & Value Objects

### 1.1 `FileContext` Enhancement

Enhance `FileContext` to persist the filesystem modification instant alongside size and category.

```rust
pub struct FileContext {
    pub scanned_root: PathBuf,
    pub file_path: PathBuf,
    pub relative_path: PathBuf,
    pub file_name: String,
    pub file_size_bytes: u64,
    pub extension: String,
    pub media_category: MediaCategory,
    /// Filesystem modification timestamp (mtime) in UTC
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<DateTime<Utc>>,
}
```

### 1.2 `FilterEvaluationResult`

Value object returned by the storage sink pre-filtering operation on a batch of discovered items.

```rust
#[derive(Debug, Clone, Default)]
pub struct FilterEvaluationResult {
    /// Items already present in database with matching size and timestamp (to skip)
    pub unmodified_items: Vec<DiscoveredMediaItem>,
    /// Items that are new or whose size/timestamp changed (to extract)
    pub items_to_extract: Vec<DiscoveredMediaItem>,
    /// Cumulative byte volume of skipped unmodified items
    pub skipped_bytes: u64,
    /// Cumulative byte volume of items to extract
    pub extract_bytes: u64,
}
```

### 1.3 `ScanProgressSnapshot` Incremental Fields

Extended progress snapshot fields tracking skipped items:

```rust
pub struct ScanProgressSnapshot {
    // ... existing fields ...
    /// Items skipped because they already exist in database and are unmodified
    pub items_skipped: u64,
    /// Cumulative byte volume skipped
    pub bytes_skipped: u64,
}
```

### 1.4 `ExecutionSummary` Incremental Breakdown

Extended execution summary reporting:

```rust
pub struct ExecutionSummary {
    // ... existing fields ...
    /// Count of files verified as already indexed and unmodified
    pub total_skipped: u64,
    /// Count of newly discovered files extracted and indexed
    pub total_newly_extracted: u64,
    /// Count of modified files re-extracted and updated
    pub total_re_extracted: u64,
    /// Whether incremental filtering was enabled or bypassed via --force
    pub force_reindex: bool,
}
```

---

## 2. Storage Port Contract

```rust
#[async_trait::async_trait]
pub trait MetadataStorageSink: Send + Sync {
    // ... existing methods ...

    /// Pre-filter a slice of discovered items, returning unmodified vs items requiring extraction.
    async fn filter_unmodified_items(
        &self,
        items: &[DiscoveredMediaItem],
    ) -> Result<FilterEvaluationResult, StorageError>;
}
```
