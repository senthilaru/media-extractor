# Data Model: Database Content & Health Audit Report

**Feature**: `020-database-content-report`  
**Date**: 2026-11-14

## Domain Entities & Value Objects

### 1. `CatalogAuditReport`
Represents the aggregated health and inventory metrics of the media catalog at a specific point in time.

| Field | Type | Description |
|---|---|---|
| `total_files` | `u64` | Total number of media documents cataloged in the database |
| `extension_counts` | `Vec<ExtensionCount>` | Distribution of media files grouped by file extension, ordered descending by count |
| `copied_to_master` | `u64` | Number of documents recognized as canonical master assets (`DeduplicationStatus::CanonicalMaster`) |
| `total_replicas` | `u64` | Total number of verified replica backup copies preserved on backup drives (`ReplicaKeep`) |
| `replica_drives` | `Vec<ReplicaDriveCount>` | Itemized breakdown of replicated assets and pending counts per replica drive |
| `yet_to_replicate` | `u64` | Number of canonical masters pending replication across backup drives |
| `duplicates_detected` | `u64` | Number of redundant duplicate assets identified for pruning (`MarkForRemoval` / `Removed`) |
| `marked_for_delete` | `u64` | Number of redundant files marked for deletion/prune (`DeduplicationStatus::MarkForRemoval`) |
| `missing_exif` | `u64` | Number of assets missing EXIF/QuickTime capture dates (`TimestampSource::FileSystemModified`) |
| `problem_files` | `u64` | Number of assets flagged with extraction or validation errors (`ExtractionResultKind::Failed` or error message present) |
| `unmastered_files` | `u64` | Number of scanned items awaiting ingestion to canonical master (`DeduplicationStatus::NeedsIngestToMaster`) |
| `generated_at` | `DateTime<Utc>` | UTC timestamp when the report was compiled |
| `recommended_action` | `Option<CommandRecommendation>` | Highest-priority suggested next CLI action based on current catalog state |

---

### 2. `ExtensionCount`
Value object representing file count for a specific file format extension.

| Field | Type | Description |
|---|---|---|
| `extension` | `String` | Normalized lowercase extension (e.g., `jpg`, `heic`, `mp4`) or `[unknown]` |
| `count` | `u64` | Number of cataloged assets with this extension |
| `percentage` | `f64` | Percentage of total catalog volume represented by this format |

---

### 3. `InspectionCategory`
Enum identifying the target metric slice for manual verification.

```rust
pub enum InspectionCategory {
    Problems,
    MissingExif,
    Duplicates,
    YetToReplicate,
    MarkedForDelete,
    Unmastered,
}
```

---

### 4. `InspectionSample`
Represents an individual sample asset retrieved for operator inspection.

| Field | Type | Description |
|---|---|---|
| `id` | `Uuid` | Database document UUID |
| `file_path` | `PathBuf` | Full filesystem path on disk |
| `file_size_bytes` | `u64` | File size reported by filesystem |
| `extension` | `String` | Normalized extension |
| `category` | `InspectionCategory` | The category under which this sample was retrieved |
| `diagnostic_detail` | `Option<String>` | Additional detail (e.g. error message, pending replica drive IDs, or duplicate master path) |

---

### 5. `CommandRecommendation`
Actionable instruction advising the operator on the next operational pipeline command.

| Field | Type | Description |
|---|---|---|
| `priority` | `u8` | Priority level (1 = highest urgency) |
| `title` | `String` | Short title (e.g. "Ingest New Assets", "Synchronize Backup Replicas") |
| `reason` | `String` | Human-readable explanation of why this action is recommended based on metrics |
| `command` | `String` | Exact copy-pasteable CLI command string (e.g. `cargo run -- sync-replicas`) |

---

## State Transition & Recommendation Flow

```
[ CatalogAuditReport Generated ]
              │
              ▼
   Any unmastered assets? ─────────► (Yes) ──► Suggest: `cargo run -- ingest`
              │ (No)
              ▼
   Any pending replicas? ──────────► (Yes) ──► Suggest: `cargo run -- sync-replicas`
              │ (No)
              ▼
   Any marked for deletion? ───────► (Yes) ──► Suggest: `cargo run -- organize` (or inspect)
              │ (No)
              ▼
   Any extraction problems? ───────► (Yes) ──► Suggest: `cargo run -- report inspect problems`
              │ (No)
              ▼
   Any missing EXIF? ──────────────► (Yes) ──► Suggest: `cargo run -- report inspect missing-exif`
              │ (No)
              ▼
    All Metrics Clean ─────────────► All in sync! No action required.
```
