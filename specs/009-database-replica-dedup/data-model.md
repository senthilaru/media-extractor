# Data Model: Database-Driven Replica-Aware Media Deduplication

**Feature**: `009-database-replica-dedup`  
**Date**: 2026-11-09  
**Status**: Completed  

---

## 1. Domain Entities & Value Objects

```
┌─────────────────────────────────┐
│       DriveConfiguration        │
│  - version: String              │
│  - groups: HashMap<String, ...> │
│  - drives: Vec<DriveDefinition> │
└───────────────┬─────────────────┘
                │ contains
                ▼
┌─────────────────────────────────┐
│        DriveDefinition          │
│  - id: String                   │
│  - group: String                │
│  - mount_path: PathBuf          │
│  - master_path: PathBuf         │
└───────────────┬─────────────────┘
                │ evaluates path against
                ▼
┌─────────────────────────────────┐
│       CanonicalPathStatus       │
│  - InMaster(Year, Date, Cat)    │
│  - OutsideMaster                │
│  - UnknownDrive                 │
└─────────────────────────────────┘
```

### `DriveConfiguration`
The top-level configuration loaded from `drives.yaml`.
- **`version`**: `String` (e.g. `"1.0"`)
- **`groups`**: Map of group name to `DriveGroupSpec`:
  - `priority`: `u32` (Higher integer = higher priority, e.g. `primary: 100`, `replica1: 80`, `replica2: 60`)
  - `description`: `Option<String>`
- **`drives`**: `Vec<DriveDefinition>`

### `DriveDefinition`
- **`id`**: `String` (Unique alphanumeric name, e.g. `"primary_main"`)
- **`group`**: `String` (References a key in `groups`, e.g. `"primary"` or `"replica1"`)
- **`mount_path`**: `PathBuf` (e.g. `/Volumes/SP-1`)
- **`master_path`**: `PathBuf` (e.g. `/Volumes/SP-1/SPSA/media/master`)

### `DeduplicationStatus` (Enum)
Represents the evaluated lifecycle state of an asset within the duplicate cluster:
- **`CanonicalMaster`**: The primary master asset residing in the canonical master structure on a primary drive.
- **`ReplicaKeep`**: An authorized replica copy residing in the canonical master structure on a replica drive.
- **`MarkForRemoval`**: A redundant duplicate copy located outside canonical master paths (or redundant non-master duplicates).
- **`NeedsIngestToMaster`**: A unique asset that exists only in unorganized/scattered folders and has no master copy yet (strictly protected from deletion).
- **`ConflictReview`**: Multiple conflicting copies exist within the master directory hierarchy, requiring manual operator resolution.
- **`Removed`**: Physical file has been confirmed deleted from disk.

### `DeduplicationMetadata` (Embedded Document in MongoDB)
Embedded inside `MediaMetadataDocument` under the field `"dedup"`:

```json
{
  "cluster_id": "848bb22d-b0df-4fc8-9f15-37397c0f16f1",
  "status": "MarkForRemoval",
  "canonical_master_id": "e6f9a0c2-124b-4b13-9a3b-28f9d0c6418b",
  "canonical_master_path": "/Volumes/SP-1/SPSA/media/master/2021/30-Nov-2021/Photos/IMG_1042.JPG",
  "drive_id": "primary_main",
  "drive_group": "primary",
  "is_in_master_path": false,
  "reclaimable_bytes": 34819200,
  "evaluated_at": "2026-11-09T03:25:00Z"
}
```

| Field | Type | Description |
|---|---|---|
| `cluster_id` | `Uuid` | Unique identifier grouping identical content copies |
| `status` | `DeduplicationStatus` | Action/role classification |
| `canonical_master_id` | `Option<Uuid>` | UUID of the designated canonical master document |
| `canonical_master_path` | `Option<PathBuf>` | File path of the canonical master file on primary drive |
| `drive_id` | `Option<String>` | Identifier of the drive where this file is located |
| `drive_group` | `Option<String>` | Group role (`primary`, `replica1`, etc.) |
| `is_in_master_path` | `bool` | Whether this copy conforms to `<MasterRoot>/<Year>/<DD-Mon-YYYY>/<Category>` |
| `reclaimable_bytes` | `u64` | Bytes freed if this duplicate file is deleted |
| `evaluated_at` | `DateTime<Utc>` | Timestamp of when the deduplication logic evaluated this record |

---

## 2. Drive Removal Manifest

Data entity generated when planning or executing cleanup for a specific mounted drive:

```json
{
  "drive_id": "primary_main",
  "mount_path": "/Volumes/SP-1",
  "drive_group": "primary",
  "total_files_to_remove": 420,
  "total_reclaimable_bytes": 142857000000,
  "removal_tasks": [
    {
      "document_id": "c71a39d8-...",
      "target_file_path": "/Volumes/SP-1/Downloads/OldBackup/IMG_1042.JPG",
      "canonical_master_path": "/Volumes/SP-1/SPSA/media/master/2021/30-Nov-2021/Photos/IMG_1042.JPG",
      "file_size_bytes": 34819200,
      "master_exists_verified": true
    }
  ]
}
```

---

## 3. Lifecycle & State Transitions

```
                    ┌─────────────────────────┐
                    │    Discovered Item      │
                    │   (Extracted to DB)     │
                    └────────────┬────────────┘
                                 │
                                 ▼
                     Global Duplicate Query
                                 │
                 ┌───────────────┴───────────────┐
                 │                               │
        Count == 1 (Unique)              Count > 1 (Duplicates)
                 │                               │
                 ▼                               ▼
       In Master Directory?             Evaluate Hierarchy:
        ├── YES -> UnreplicatedMaster   ├── Primary Master Path -> CanonicalMaster
        └── NO  -> NeedsIngestToMaster  ├── Replica Master Path -> ReplicaKeep
                                        ├── Multiple in Master  -> ConflictReview
                                        └── Outside Master Path -> MarkForRemoval
                                                                        │
                                                                   Clean Command
                                                                   (--execute)
                                                                        ▼
                                                                     Removed
```
