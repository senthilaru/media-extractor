# Technical Research: Incremental Media Scan & Database Change Caching

**Feature**: `008-incremental-media-scan`  
**Date**: 2026-11-09  
**Status**: Completed  

---

## 1. Batch Change Detection & Storage Query Strategy

### Context
When scanning a large media archive (such as 1 TB with 13,000+ photos and videos), re-scanning the same path without pre-filtering forces the pipeline to perform container demuxing, EXIF decoding, duration parsing, and thumbnail rendering for every file again before reaching the database upsert layer. Because the unique index on `file_context.file_path` already exists in MongoDB, we can detect unmodified assets before extraction.

### Decision
1. **Pre-Extraction Filter Port**:
   Extend `MetadataStorageSink` trait with:
   ```rust
   async fn filter_unmodified_items(
       &self,
       items: &[DiscoveredMediaItem],
   ) -> Result<Vec<DiscoveredMediaItem>, StorageError>;
   ```
   Or return a partitioned result distinguishing `unmodified` vs `to_extract` items.
2. **Bulk B-Tree Query**:
   Query MongoDB using an `$in` expression over the 500 paths in the batch:
   ```rust
   doc! {
       "file_context.file_path": { "$in": paths }
   }
   ```
   With projection restricted to `{ "file_context.file_path": 1, "file_context.file_size_bytes": 1, "file_context.modified_at": 1 }`.
3. **Change Detection Rule**:
   A file is considered unmodified if and only if:
   - Its canonical path exists in the database record.
   - Its current filesystem byte length equals `file_context.file_size_bytes`.
   - Its current filesystem modification timestamp (`mtime`) matches `file_context.modified_at` (within a 1-second boundary to handle filesystem timestamp truncation across OS boundaries).
   If any attribute differs or the record is missing, the file is scheduled for extraction.

### Rationale
Executing a single indexed query per batch of 500 items requires $\approx 5\text{--}15$ ms of database latency. For 13,000 files, total pre-filtering query time is under $0.5$ seconds, saving $> 40$ minutes of repetitive decoding and disk I/O.

### Alternatives Considered
- **Per-file queries**: Issuing 13,000 individual `find_one` roundtrips creates massive latency overhead.
- **Local SQLite cache**: Adds an unwanted local file dependency when MongoDB is already the source of truth for the archive.

---

## 2. Pipelined Concurrency Integration & Backpressure

### Context
The extraction pipeline (`ExtractionPipeline`) receives batches of `DiscoveredMediaItem` from `ScanEngine` and dispatches them across worker threads.

### Decision
1. In `ExtractionPipeline::execute`, before acquiring worker permits for a batch:
   - If a storage sink is configured and `--force` is not requested, run `storage_sink.filter_unmodified_items(&batch.items).await`.
   - Separate unmodified items from items needing extraction.
   - For unmodified items:
     - Increment `progress_tracker.record_skipped(count, bytes)`.
     - Bypasses extraction workers completely.
   - For remaining items (new or modified):
     - Pass to worker pool for EXIF/container decoding and hashing.
2. If all items in a batch are unmodified:
   - Worker permit acquisition and compute dispatch are completely bypassed.

### Rationale
Keeps worker threads free for any newly discovered or modified files while immediately advancing the progress bar for cached items.

---

## 3. CLI Interface & Force Re-Index Controls

### Context
Operators occasionally update decoders, modify metadata requirements, or wish to rebuild indexes from scratch without manual database drops.

### Decision
Add a CLI argument and environment variable:
- `--force` / `--reindex` (flag, default `false`, env: `MEDIA_FORCE_REINDEX`):
  When set to `true`, disables incremental pre-filtering and extracts all discovered files.

### Rationale
Provides explicit operator escape hatch and aligns with standard CLI patterns (such as `make -B` or `cargo build --force`).
