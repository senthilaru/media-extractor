# Research: Automated Master Ingestion and Source Deletion Marking

**Feature**: `010-ingest-to-master`  
**Date**: 2026-11-09  
**Status**: Completed  

---

## 1. Streaming I/O, Buffer Management & Bitwise Verification

### Context
Ingesting unmastered assets from a secondary external drive to the primary master drive requires moving large video files (often 10GB–50GB 4K video clips) and multi-gigabyte RAW image collections across USB 3 / Thunderbolt buses. Buffering entire files into memory would violate constitutional bounds (<100MB RAM) and cause out-of-memory crashes on Apple Silicon M1 unified memory architecture. Furthermore, the system must guarantee zero data loss: a destination file must pass bitwise validation before the source file is marked for deletion.

### Decision
Implement an asynchronous chunked streaming copier with parallel bitwise BLAKE3 verification:
1. **Streaming Copy with Bounded Buffer**: Use `tokio::fs::File` with a 1MB streaming buffer (`tokio::io::copy_buf` or chunked streaming loop).
2. **Transfer Rate Telemetry**: Emit real-time byte counters and transfer speeds (MB/s) through `indicatif` progress bar / telemetry events.
3. **Bitwise Integrity Check**:
   - Compare destination file size against source file size.
   - If source document already has `fingerprint.cryptographic_hash` (BLAKE3), compute BLAKE3 digest of copied destination file using memory-mapped Rayon hasher (`Blake3MediaHasher`).
   - If hash matches: transfer is 100% verified.
   - If size or hash mismatches: immediately abort, delete incomplete destination file, and preserve source document as `NeedsIngestToMaster`.

### Rationale
- 1MB buffer ensures memory stays well under 5MB even when multiple workers run, while achieving maximum external SSD throughput (500MB/s–2000MB/s).
- BLAKE3 hash comparison guarantees that no byte corruption occurred during USB controller transfer.
- Cleaning up failed/partial copies prevents orphaned half-written files on the primary master disk.

### Alternatives Considered
- Direct `std::fs::copy`: OS-level copy does not report fine-grained progress callbacks and does not calculate hashes inline.
- Read-then-write whole file: Unbounded memory allocation, fails on large video files.

---

## 2. Canonical Destination Path Generation & Collision Resolution

### Context
Unmastered assets marked `NeedsIngestToMaster` originate from messy folders (e.g. `/Volumes/Drive-3/scratch_dump/DCIM/IMG_0042.JPG`). In the primary master archive, they must be organized into:
`<MasterRoot>/<Year>/<DD-Mon-YYYY>/<CategoryFolder>/<filename>`
Example:
`/Volumes/SP-1/SPSA/media/master/2021/30-Nov-2021/Photos/IMG_0042.JPG`

### Decision
1. **Target Date Derivation**:
   - Primary: Extract `year`, `month`, `day` from the document's `TemporalMetadata` (EXIF `DateTimeOriginal`, QuickTime creation time).
   - Fallback 1: Filesystem `modified_at` timestamp.
   - Fallback 2: Current year with date folder `Unsorted` (e.g. `2026/Unsorted/Photos/`).
   - Format: `<Year>` is 4 digits (e.g. `2021`), `<DateFolder>` is formatted as `DD-Mon-YYYY` (e.g. `30-Nov-2021` using English 3-letter month abbreviations matching Feature 009 canonical patterns).
2. **Category Subfolder**:
   - `MediaCategory::Photo` $\rightarrow$ `Photos`
   - `MediaCategory::RawPhoto` $\rightarrow$ `PhotosRaw`
   - `MediaCategory::Video` $\rightarrow$ `Videos`
3. **Collision Handling**:
   - If target path does not exist: write directly.
   - If target path exists on disk:
     - Compute/check cryptographic hash:
       - If identical: Destination already possesses this exact asset! Skip copy, link source to existing master, and update source status to `MarkForRemoval`.
       - If different: Append collision suffix before file extension: `<stem>_1.<ext>`, `<stem>_2.<ext>`, etc., until an unused path is found.

### Rationale
- Strictly aligns with the canonical pattern verified by `CanonicalPathMatcher`.
- Non-destructive collision handling guarantees that two different photos named `IMG_0001.JPG` taken on the same day by different cameras are both preserved.

---

## 3. Database State Transition & Master Registration Workflow

### Context
When a file is ingested, the database must accurately track both the new master asset on the primary drive and the status transition of the source file.

### Decision
Execute a two-document transactional or sequential update in MongoDB:
1. **Register New Master Document**:
   - Construct a new `MediaMetadataDocument` for the target path (or clone the source document with updated `id = Uuid::new_v4()`, updated `file_context` pointing to primary master path, and `dedup` metadata).
   - Set `dedup`:
     ```json
     {
       "cluster_id": "<existing_or_new_cluster_id>",
       "status": "CanonicalMaster",
       "canonical_master_id": "<new_master_id>",
       "canonical_master_path": "<target_file_path>",
       "drive_id": "<target_primary_drive_id>",
       "drive_group": "primary",
       "is_in_master_path": true,
       "reclaimable_bytes": 0,
       "evaluated_at": "<now>"
     }
     ```
   - Persist to MongoDB via `save_batch` (idempotent upsert by path).
2. **Update Source Document**:
   - Set `dedup.status = DeduplicationStatus::MarkForRemoval`.
   - Set `dedup.canonical_master_id = Some(new_master_id)`.
   - Set `dedup.canonical_master_path = Some(target_file_path)`.
   - Set `dedup.reclaimable_bytes = source_file_size`.
   - If `--delete-source` was requested and physical file removal succeeded: set `dedup.status = DeduplicationStatus::Removed`.
   - Update in MongoDB via `update_dedup_batch`.

### Rationale
- Preserves full auditability in MongoDB.
- Once marked `MarkForRemoval`, the existing `metadata-extractor dedup prune --drive <ID>` command naturally recognizes the unmastered source files as verified candidates ready for drive cleanup.

---

## 4. Multi-Drive Orchestration & Fail-Fast Safety Checks

### Context
The operator connects two external USB/Thunderbolt drives at once. Mount paths can fail, disconnect, or have insufficient disk capacity.

### Decision
Implement strict pre-flight validation in `MasterIngestService`:
1. **Source Drive Check**: Resolve `--source-drive <ID_OR_MOUNT>` against `drives.yaml`. Verify drive mount path exists on filesystem and is accessible.
2. **Target Drive Check**: Resolve `--target-drive` (or default to the first configured drive in group `primary`). Verify primary mount path and `master_path` exist on filesystem and are writable.
3. **Available Disk Space Check**: Query filesystem free space (`statvfs` on Unix/macOS) on target drive. Verify available bytes exceed total transfer bytes with at least 1GB safety margin.
4. **Dry-Run Planning**: Support `--dry-run` to compile the `MasterIngestPlan` and display all mapping paths without copying bytes or altering MongoDB records.

### Rationale
- Fails fast in under 10ms if an operator forgets to plug in the primary drive or selects an unmounted volume.
- Prevents drive space exhaustion mid-transfer.
