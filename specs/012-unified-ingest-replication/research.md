# Phase 0 Research: Unified Master Ingestion and Multi-Replica Synchronization

**Feature Branch**: `012-unified-ingest-replication`  
**Date**: 2026-11-10  
**Context**: Design and architecture for universal asset classification (unique + duplicate), bitwise-verified master ingestion, multi-replica fanout synchronization, and elimination of redundant transfer/verification logic.

---

## 1. Universal Content Classification (Unique & Duplicate Media)

### Problem
Previously, catalog deduplication evaluated only duplicate clusters (`count > 1` matching cryptographic hash). Unique assets discovered on external drives or staging folders were completely omitted from classification and never scheduled for ingestion. Furthermore, when duplicate clusters had zero copies in the canonical master path, the previous implementation marked all copies as `NeedsIngestToMaster`, creating duplicate ingestion attempts.

### Decision
Unify catalog classification under a single evaluation rule that operates on **all content groups** (both unique assets where `count == 1` and duplicate clusters where `count > 1`):
1. **Catalog Query**: Implement `query_all_content_groups()` on `MetadataStorageSink` using MongoDB aggregation grouping on `fingerprint.cryptographic_hash`. Unlike `query_duplicate_groups()`, this query does not filter with `$match: { count: { $gt: 1 } }`. It streams or yields all content groups with hash key and member document vectors.
2. **Unified State Evaluation Algorithm (`evaluate_content_group`)**:
   - Inspect each document against `CanonicalPathMatcher` to resolve drive group, priority, and `is_in_master`.
   - **Case A: Copy exists in Primary Master** (`drive_group == "primary"` && `is_in_master`):
     - Designate the primary master copy as `CanonicalMaster` (if multiple exist in primary master, flag as `ConflictReview`).
     - Designate any copies in canonical paths on secondary/replica drives as `ReplicaKeep`.
     - Designate all other copies (outside canonical master paths or unneeded copies) as `MarkForRemoval`.
   - **Case B: No copy exists in Primary Master**:
     - Rank all copies in the group by `group_priority` (descending), then existing canonical path structure, then oldest indexed timestamp.
     - Designate the single highest-ranked candidate as `NeedsIngestToMaster`.
     - Designate all remaining copies in the cluster as `MarkForRemoval` with reference to the pending ingest target.
3. **Replica Drive Pre-existing Copy Handling**:
   - If an asset is already in canonical path on a replica drive (`drive_group == "replica1"` && `is_in_master`) but missing on primary master, it is chosen as the ingestion source (`NeedsIngestToMaster`).
   - Upon successful ingestion and verification on the primary master, the original file on the replica drive transitions directly to `ReplicaKeep` rather than being deleted or marked for removal.

### Rationale
- Treats unique and duplicate assets under an identical state-transition model: every unique content hash must have exactly 1 `CanonicalMaster` on primary, 1 `ReplicaKeep` per configured replica drive, and 0 redundant copies elsewhere.
- Eliminates duplicate ingestion tasks: exactly 1 file per content hash is transferred to primary master.
- Fully backwards-compatible with existing `DeduplicationStatus` enum and MongoDB document schema.

### Alternatives Considered
- *Separate query and processor for unique assets*: Rejected because it introduces dual maintenance overhead and risks race conditions/divergent rules between unique and duplicate classification.
- *In-memory classification without database aggregation*: Rejected because loading all catalog documents into memory violates Constitution Principle I (bounded memory on millions of media files).

---

## 2. Multi-Replica Synchronization & Fanout Engine

### Problem
The existing codebase supports ingesting assets into the primary master drive, but lacks automated fanout to secondary and tertiary backup drives (`replica1`, `replica2`, ...). Backup administrators had to manually duplicate drives or run ad-hoc rsync scripts, risking inconsistent catalog metadata and unmonitored silent transfers.

### Decision
Build a dedicated `ReplicaSyncService` (integrated with `UnifiedFileTransferService`) that mirrors `CanonicalMaster` assets to all configured replica drives:
1. **Drive Discovery**:
   - Read replica drive definitions from `DriveConfiguration` (e.g., drives where `group` starts with `"replica"` or priority < primary).
   - Check each replica drive's mount status (`mount_path.exists()`).
   - If a replica drive is offline/unmounted, mark it as `Offline` and track pending replication count; do not fail or interrupt synchronization to other mounted replica drives.
2. **Plan Compilation (`compile_replication_plan`)**:
   - For every `CanonicalMaster` document in the catalog:
     - Determine the relative canonical subpath: `<Year>/<DD-Mon-YYYY>/<CategoryFolder>/<filename>`.
     - For each mounted replica drive $R$:
       - Target path: `R.master_path.join(canonical_subpath)`.
       - Check if catalog or destination filesystem already possesses a verified copy (`ReplicaKeep` on drive $R$ with matching size and cryptographic hash).
       - If missing, emit a `ReplicaSyncTask` { `canonical_master_id`, `source_path`, `target_path`, `replica_drive_id`, `file_size_bytes`, `cryptographic_hash` }.
3. **Execution & Catalog Registration**:
   - Execute file transfers using bounded streaming buffers (1MB).
   - Verify destination file byte size and BLAKE3 cryptographic hash against master.
   - Upon verification success:
     - Clone source master metadata into a new `MediaMetadataDocument` with `id: Uuid::new_v4()`.
     - Set `dedup = DeduplicationMetadata { status: ReplicaKeep, canonical_master_id: Some(master_id), drive_id: Some(replica_drive_id), drive_group: Some(replica_group), is_in_master_path: true, reclaimable_bytes: 0 }`.
     - Bulk-upsert replica documents into MongoDB sink.

### Rationale
- Guarantees true multi-replica redundancy: primary master is the single source of truth, and all configured replica drives mirror the exact relative structure and verified bitwise contents.
- Tolerates partial hardware connectivity: backup drives can be attached and synchronized independently without requiring all drives to be simultaneously online.

### Alternatives Considered
- *Direct filesystem traversal of primary master instead of catalog*: Rejected because the database already has indexed metadata, pre-computed hashes, and temporal categories. Re-scanning filesystem would be slow and violate Hexagonal architecture.
- *External system rsync invocation*: Rejected because it introduces external process dependencies, cannot update MongoDB catalog records, bypasses Rust safety guarantees, and fails to emit structured telemetry.

---

## 3. Unified File Transfer & Verification Architecture (Eliminating Redundancy)

### Problem
`MasterIngestService` previously implemented inline directory creation, stream copying, collision path resolution, and hash verification. Creating a replica synchronization engine by copying these methods would introduce substantial code duplication, conflicting error handling, and maintenance drift.

### Decision
Extract and consolidate all file transfer, verification, collision avoidance, and path generation logic into a shared component: `UnifiedFileTransferService` (or `FileTransferManager`):
1. **Shared Primitives**:
   - `stream_copy_verified(source: &Path, target: &Path, expected_size: u64, expected_hash: Option<&str>, verify_hash: bool) -> Result<u64, DeduplicationError>`
     - Uses 1MB streaming buffer via `tokio::io`.
     - Verifies byte size post-copy.
     - Computes BLAKE3 hash via `Blake3MediaHasher` and verifies against expected hash.
     - Automatically purges partial/corrupted destination file upon any failure before returning error.
   - `resolve_target_path(base_target: &Path, source_hash: Option<&str>, source_size: u64) -> PathResolution`
     - Evaluates whether target file exists.
     - If exists and size/hash match: returns `PathResolution::IdenticalExists(path)`.
     - If exists and content differs: appends deterministic collision suffix `_1`, `_2`, ... and returns `PathResolution::CollisionResolved(new_path)`.
     - If does not exist: returns `PathResolution::Available(path)`.
   - `build_canonical_subpath(temporal: &TemporalMetadata, mtime: Option<DateTime<Utc>>, category: MediaCategory, filename: &str) -> PathBuf`
     - Consistently constructs `<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}/<Filename>`.
     - Includes fallback handling for missing or unparseable capture dates (`UnknownDate/<Category>`).
2. **Reusability**:
   - Both `MasterIngestService` (source -> primary) and `ReplicaSyncService` (primary -> replicas) use these identical methods.
   - Any future transfer operation (e.g. drive migration or cloud backup) will automatically reuse this robust, verified pipeline.

### Rationale
- Completely eliminates redundant code logic (satisfying FR-013 and User Story 4).
- Concentrates security and data-integrity critical code (file I/O, hash verification, cleanup) in a single audited location.

---

## 4. Live Progress Telemetry & Timing Metrics

### Problem
Constitution Principle III and Performance Standards mandate live progress indicators and elapsed time taken metrics on all I/O and external service operations, with zero unmonitored silent execution.

### Decision
- Integrate `IndicatifTerminalProgress` / structured progress heartbeats into `UnifiedFileTransferService`, `MasterIngestService`, and `ReplicaSyncService`.
- Emit live metrics during batch transfers:
  - Items completed vs. planned
  - Data volume transferred (MB/GB) vs. total
  - Instantaneous and average throughput rate (MB/sec, items/sec)
  - Elapsed duration and estimated time of completion (ETA)
- Support `--quiet` (suppresses ANSI bars) and `--json-telemetry` (emits discrete NDJSON progress objects for automation).
- Final reports (`IngestExecutionReport`, `ReplicaSyncReport`) include full execution timing (`elapsed_seconds`, `bytes_transferred`, `files_transferred`, `throughput_mb_s`).

---

## 5. CLI Interface Design & Command Workflows

### Decision
Support both unified sequential workflows and targeted single-operation commands:
1. `metadata-extractor ingest`:
   - Adds `--replicate` flag (default: false).
   - If `--replicate` is specified, upon completion of master ingestion, the system automatically invokes replica synchronization to fan out newly ingested assets to all connected replica drives.
2. `metadata-extractor sync-replicas` (visible alias: `replicate`):
   - Standalone command dedicated to mirroring canonical masters to replica drives.
   - Options:
     - `--replica <ID>`: Target a specific replica drive (default: all configured mounted replica drives).
     - `--dry-run`: Preview planned replica transfers without copying.
     - `--no-verify-hash`: Skip cryptographic hash check (byte size check retained).
     - `--config <PATH>`: Path to `drives.yaml`.
3. `metadata-extractor dedup --all` (or universal classification):
   - Evaluates all catalog records (both unique and duplicates) against primary and replica drive hierarchies, tagging `NeedsIngestToMaster`, `CanonicalMaster`, `ReplicaKeep`, and `MarkForRemoval`.
