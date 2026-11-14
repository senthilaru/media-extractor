# Technical Research: Database Content & Health Audit Report

**Feature**: `020-database-content-report`  
**Date**: 2026-11-14  
**Status**: Completed

## 1. MongoDB Aggregation Pipeline vs. Multiple Count Queries

### Decision
Use a single unified MongoDB `$facet` aggregation pipeline with projected match stages for the status metrics and an unwound `$group` for file extension frequency, executed via the database driver directly in `MongoDbSink`.

### Rationale
- **Single Roundtrip**: Querying a 1M+ document collection with 8 separate `count_documents` calls incurs 8 network round-trips and repeated BSON deserialization overhead. A `$facet` pipeline executes server-side in a single network round-trip.
- **Minimal Memory Overhead**: Aggregation pipelines compute counts and groupings in MongoDB's native engine, streaming back only the compact summary document rather than transferring raw document records across the wire.
- **Index Utilization**:
  - Total count: collection metadata / clustered index.
  - Extension grouping: uses index on `file_context.extension`.
  - Ingested / Copied: `dedup.status = "CanonicalMaster"` utilizes the compound index on `dedup.status`.
  - Pending replication: `dedup.replication.status` in `["Pending", "PartiallyReplicated"]`.
  - Duplicates: `dedup.status` in `["ReplicaKeep", "MarkForRemoval", "Removed"]` or cluster size > 1.
  - Marked for removal: `dedup.status = "MarkForRemoval"`.
  - Missing EXIF: `temporal.source = "FileSystemModified"`.
  - Problematic items: `extraction_status.status = "Failed"` or `extraction_status.error_message != null`.

### Alternatives Considered
- *Multiple sequential `count_documents` calls*: Simple to implement in Rust, but increases latency proportionally to network RTT and executes multiple index scans instead of an optimized single pass.
- *Client-side in-memory aggregation*: Fetching all document IDs and statuses to compute counts in Rust Rayon threads. Rejected because streaming millions of documents consumes massive client memory and saturates network bandwidth, directly violating Constitution Principle I (Bounded Memory).

---

## 2. Top-N Sample Inspection Strategy

### Decision
Implement targeted sample queries using `find()` with selective index matching, strict projection (`projection: {"file_context.file_path": 1, "file_context.file_name": 1, "extraction_status": 1, "dedup": 1, "_id": 1}`), and `.limit(limit.unwrap_or(3))`.

### Rationale
- When an operator requests the top 3 items for manual verification (e.g. `report inspect problems` or `report --inspect problems`), only the file path, error description, or status context needs to be retrieved.
- Projecting only `file_path` and category-specific diagnostic fields avoids fetching multi-kilobyte metadata documents (such as embedded EXIF, full dimensions, and perceptual hashes).
- Combined with `limit(3)`, MongoDB stops searching after finding 3 matches using the indexed fields, resulting in sub-millisecond execution times.

### Alternatives Considered
- *Reusing full document queries*: Fetching the entire `MediaMetadataDocument` and discarding fields on the client. Rejected to conserve I/O bandwidth and minimize BSON decoding overhead.

---

## 3. CLI Subcommand and Argument Structure

### Decision
Define a `Report` command under `enum Commands` in `src/main.rs`:
```bash
cargo run -- report [OPTIONS] [SUBCOMMAND]
```
With the following syntax:
- `cargo run -- report`: Runs the full database audit report, displaying total files, extension breakdown table, status category tallies, and the suggested next command.
- `cargo run -- report inspect <CATEGORY>` (or `cargo run -- report --inspect <CATEGORY> --limit 3`): Lists the top N (default 3) file paths matching the category (`problems`, `missing-exif`, `duplicates`, `unreplicated`, `marked-for-delete`, `unmastered`).
- `--format <text|json>`: Outputs human-readable terminal formatting (default) or machine-readable JSON for scripting.

### Rationale
- Follows existing Clap derive patterns in `src/main.rs` (`Scan`, `Extract`, `Dedup`, `Ingest`, `SyncReplicas`, `Organize`).
- Aliases `status` and `audit` to `report` for high CLI usability.
- Subcommand / flag duality provides both quick inline inspection (`--inspect`) and explicit subcommand navigation (`inspect`).

### Alternatives Considered
- *Separate top-level `inspect` command*: Adds clutter to top-level help and disconnects inspection from reporting. Grouping under `report` keeps audit functionality cohesive.

---

## 4. Next-Action Recommendation Heuristic

### Decision
Implement a deterministic priority-ranked decision tree that evaluates the computed audit metrics in order of operational dependency:
1. **Unmastered Ingestion Needed** (`unmastered > 0`):
   *Action*: Suggest `cargo run -- ingest` or `cargo run -- organize` to ingest newly scanned secondary files into the canonical master hierarchy.
2. **Pending Replicas** (`yet_to_replicate > 0`):
   *Action*: Suggest `cargo run -- sync-replicas` to synchronize canonical masters to configured backup drives.
3. **Pending Deletion / Pruning** (`marked_for_delete > 0`):
   *Action*: Suggest `cargo run -- organize` or review with `cargo run -- report inspect marked-for-delete`.
4. **Extraction Problems** (`problems > 0`):
   *Action*: Suggest inspecting failed items with `cargo run -- report inspect problems` to review corruption or permission issues.
5. **Missing EXIF / Metadata** (`missing_exif > 0`):
   *Action*: Suggest checking items with `cargo run -- report inspect missing-exif` for manual timestamp tagging or sidecar creation.
6. **All Healthy / In Sync** (`all pending == 0`):
   *Action*: State that the library is fully synchronized, organized, and healthy.

### Rationale
- Mirrors the exact lifecycle stages defined in Constitution and the Unified Organize Pipeline (`018`).
- Prevents operators from performing destructive or premature operations out of sequence.
