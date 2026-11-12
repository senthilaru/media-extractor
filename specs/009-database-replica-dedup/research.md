# Technical Research: Database-Driven Replica-Aware Media Deduplication

**Feature**: `009-database-replica-dedup`  
**Date**: 2026-11-09  
**Status**: Completed  

---

## 1. Drive Configuration Schema & Format

### Context
The operator manages 10+ external hard drives. These drives are partitioned into logical role tiers:
- `primary`: The authoritative storage hosting the organized master media archive.
- `replica1`, `replica2`, ...: Mirror / backup drives holding identical replica copies of the master archive.
- Drives may mount under various paths (e.g. `/Volumes/SP-1`, `/Volumes/SP-2`, `/Volumes/Backup-4TB`, `/mnt/storage`). Each drive in a group specifies its mount point and the relative path to its canonical master archive root (e.g., `SPSA/media/master`).

### Decision
Use a clean YAML configuration format (with optional fallback to TOML or JSON). The configuration file defines:
1. `groups`: Named drive groups (`primary`, `replica1`, `replica2`, ...).
2. `drives`: List of physical drive definitions:
   - `id`: Human-readable identifier (e.g. `primary_main`, `replica1_wd8tb`).
   - `group`: Target group name (`primary`, `replica1`, etc.).
   - `mount_path`: File system mount path (e.g. `/Volumes/SP-1`).
   - `master_path`: Full or mount-relative path to the canonical master folder (e.g. `SPSA/media/master` or `/Volumes/SP-1/SPSA/media/master`).
3. `rules`: Canonical path rules, such as date format pattern (`YYYY/DD-Mon-YYYY/{Videos,Photos,PhotosRaw}`).

```yaml
version: "1.0"
groups:
  primary:
    priority: 100
    description: "Primary home media server master storage"
  replica1:
    priority: 80
    description: "First complete mirror backup"
  replica2:
    priority: 60
    description: "Cold offline off-site backup"

drives:
  - id: "wd_14tb_primary"
    group: "primary"
    mount_path: "/Volumes/SP-1"
    master_path: "/Volumes/SP-1/SPSA/media/master"

  - id: "seagate_14tb_rep1"
    group: "replica1"
    mount_path: "/Volumes/SP-2"
    master_path: "/Volumes/SP-2/SPSA/media/master"
```

### Rationale
- YAML is human-readable, easily edited by operators without technical tools, and natively serializable via `serde_yaml`.
- Prioritized groups allow unambiguous master resolution when duplicates span multiple drives.

---

## 2. Canonical Master Path Parser & Hierarchy Rules

### Context
The user's master folder structure is structured as:
`.../master/<Year>/<DD-Mon-YYYY>/<CategoryFolder>/<filename>`
Example:
`/Volumes/SP-1/SPSA/media/master/2021/30-Nov-2021/Videos/VID_0001.MOV`
`/Volumes/SP-1/SPSA/media/master/2021/30-Nov-2021/Photos/IMG_1042.JPG`
`/Volumes/SP-1/SPSA/media/master/2021/30-Nov-2021/PhotosRaw/IMG_1042.CR3`

### Decision
Implement a zero-allocation canonical path matcher:
1. Check if `file_context.file_path` starts with a configured drive's `master_path`.
2. Extract the relative components following `master_path`.
3. Validate that the relative path matches:
   - `<Year>`: 4-digit integer (e.g. `2021`).
   - `<DateFolder>`: `DD-Mon-YYYY` (e.g. `30-Nov-2021` where month is 3-letter English month or numerical fallback `YYYY-MM-DD`).
   - `<Category>`: Exactly `Videos`, `Photos`, or `PhotosRaw`.
4. If all components match, the file is classified as **`InCanonicalMaster`**.
5. Otherwise, the file is classified as **`OutsideCanonicalMaster`** (e.g., in a scratch folder, camera dump, desktop, or unorganized directory).

### Rationale
- Strict path validation ensures that files arbitrarily copied to root directories or messy subfolders are never erroneously recognized as canonical masters.
- Matches existing `TemporalMetadata` calendar facets (`year`, `month`, `day`) already stored in MongoDB.

---

## 3. Database Query & Duplicate Clustering Strategy

### Context
MongoDB contains metadata records for assets across all 10 drives. Holding all 50,000+ asset models simultaneously in memory could be done, but streaming or indexed batching is more scalable.

### Decision
Use a 2-stage database aggregation & update pipeline:
1. **Stage 1 (Find Duplicate Hashes)**:
   Issue an aggregation pipeline to find all non-empty cryptographic hashes having count $> 1$:
   ```javascript
   db.media_assets.aggregate([
     { $match: { "fingerprint.cryptographic_hash": { $ne: null } } },
     {
       $group: {
         _id: "$fingerprint.cryptographic_hash",
         count: { $sum: 1 },
         doc_ids: { $push: "$_id" }
       }
     },
     { $match: { count: { $gt: 1 } } }
   ]);
   ```
2. **Stage 2 (Evaluate Cluster & Assign Roles)**:
   For each duplicate hash group:
   - Fetch the matching document records in bulk via `{ "_id": { "$in": doc_ids } }`.
   - Map each file path to its configured drive and determine if it is inside the canonical master path.
   - **Precedence Ranking**:
     1. Files in `primary` group inside canonical master path $\rightarrow$ **`CanonicalMaster`** (Primary Master).
     2. If multiple exist in primary master path $\rightarrow$ flag as `MasterConflict` for operator review.
     3. Files in `replicaX` group inside canonical master path $\rightarrow$ **`ReplicaKeep`**.
     4. Files outside canonical master path (on either primary or replica drives) $\rightarrow$ **`MarkForRemoval`**.
     5. If an asset has NO copies in any master path at all $\rightarrow$ **`NeedsIngestToMaster`** (protected from deletion!).
3. **Stage 3 (Bulk Update MongoDB)**:
   Issue bulk `update_one` writes setting:
   ```json
   {
     "dedup": {
       "status": "MarkForRemoval",
       "cluster_id": "<uuid>",
       "canonical_master_id": "<master_doc_id>",
       "canonical_master_path": "/Volumes/SP-1/SPSA/media/master/2021/30-Nov-2021/Photos/IMG_1042.JPG",
       "drive_group": "primary",
       "is_in_master_path": false,
       "reclaimable_bytes": 34819200,
       "evaluated_at": "2026-11-09T03:25:00Z"
     }
   }
   ```

### Rationale
- MongoDB executes the duplicate grouping index scan in milliseconds over the indexed `fingerprint.cryptographic_hash`.
- Memory usage is strictly bounded: only active duplicate clusters are processed in batches.
- Unique assets with count $= 1$ are completely ignored by the duplicate query, keeping execution fast.

---

## 4. Drive-by-Drive Safe Deletion Workflow

### Context
The operator cannot connect all 10 drives simultaneously. They want to connect Drive 3, run cleanup, unplug Drive 3, connect Drive 4, and repeat.

### Decision
1. **Manifest Export / Query**:
   `metadata-extractor dedup --manifest-for-drive <drive_id_or_mount>` or direct query:
   Find all documents where:
   - `file_context.file_path` starts with `<mount_path>`.
   - `dedup.status == "MarkForRemoval"`.
2. **Double-Confirmation Master Verification**:
   Before unlinking any file on disk:
   - Check that `dedup.canonical_master_path` is explicitly recorded.
   - Verify the canonical master record exists in MongoDB.
   - (Optional if master drive is mounted: verify file exists on disk).
3. **Dry-Run Default**:
   Running without `--execute` or `--delete` operates in `--dry-run` simulation mode, displaying the file table and total reclaimable gigabytes without touching disk.
4. **Physical Deletion**:
   When `--execute` is specified, files are deleted, and their database record status is updated to `Removed`.

### Rationale
- Prevents catastrophic accidental data loss.
- Allows operators to perform incremental disk cleanups at their own pace.
