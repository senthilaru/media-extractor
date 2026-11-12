# Contract: CLI Interface for Database Deduplication & Drive Pruning

**Feature**: `009-database-replica-dedup`  
**Contract Type**: CLI & Terminal Output Interface  
**Status**: Approved  

---

## 1. CLI Commands & Arguments

### A. Run Database Deduplication & Mark Duplicates
```bash
metadata-extractor dedup [OPTIONS]
```

| Argument | Flag | Environment Variable | Default | Description |
|---|---|---|---|---|
| `--config` | `-c` | `MEDIA_DRIVES_CONFIG` | `drives.yaml` | Path to drive configuration YAML/TOML file |
| `--dry-run` | — | — | `false` | Evaluate and preview classifications without updating MongoDB |
| `--similarity-threshold` | — | — | `10` | Max visual Hamming distance for photo near-duplicates |
| `--mode` | — | — | `exact` | Deduplication mode: `exact` (cryptographic bitwise) or `all` (+ perceptual photos) |
| `--mongodb-uri` | `--db-uri` | `MONGODB_URI` | — | MongoDB connection string |
| `--collection` | `--db-collection` | `MONGODB_COLLECTION` | `media_assets` | Target collection |

---

### B. Clean / Prune Marked Duplicates on a Specific Mounted Drive
```bash
metadata-extractor dedup prune --drive <DRIVE_ID_OR_MOUNT> [OPTIONS]
```

| Argument | Flag | Default | Description |
|---|---|---|---|
| `--drive` | `-d` | **Required** | Drive ID or mount path to prune (e.g. `wd_14tb_primary` or `/Volumes/SP-1`) |
| `--execute` | — | `false` | Actually delete files on disk (defaults to safe dry-run preview if omitted) |
| `--require-master-disk` | — | `false` | Require canonical master drive to be mounted and verified on disk before deletion |
| `--manifest-out` | `-o` | — | Save removal audit manifest to JSON file |

---

## 2. Output Reporting Contracts

### Global Deduplication Summary Output
```text
================================================================================
MEDIA DEDUPLICATION & REPLICA AUDIT SUMMARY
================================================================================
Configuration:             drives.yaml (2 groups, 3 drives)
Database:                  mongodb://localhost:27017/media_index_db (media_assets)
Mode:                      Database-Driven Exact Bitwise
Total Records Evaluated:   54,320
Duplicate Clusters Found:  1,420
Total Duplicate Files:     3,120
Reclaimable Storage:       412.50 GB
--------------------------------------------------------------------------------
Classification Breakdown:
  - Canonical Masters:     1,420 (Preserved in primary master path)
  - Replica Approved:      1,420 (Preserved in replica master path)
  - Marked For Removal:    1,700 (Redundant copies outside master paths)
  - Needs Ingest to Master:   85 (Unique unmastered files - protected!)
  - Master Conflicts:          0 (Zero conflicting copies in master hierarchy)
--------------------------------------------------------------------------------
Reclaimable by Drive:
  - wd_14tb_primary (/Volumes/SP-1):       120 files (18.40 GB)
  - toshiba_8tb_scratch (/Volumes/SP-3):  1,580 files (394.10 GB)
Database Update:           Successfully marked 54,320 records in MongoDB.
================================================================================
```

### Drive Pruning Preview Output (`--dry-run`)
```text
[DRY-RUN] Pruning candidates for drive: toshiba_8tb_scratch (/Volumes/SP-3)
Target Drive Mounted:      YES (/Volumes/SP-3)
Files Marked for Removal:  1,580
Reclaimable Space:         394.10 GB

Verified Masters:
  [OK] /Volumes/SP-3/OldDump/IMG_1042.JPG (34.8 MB) -> Master: /Volumes/SP-1/SPSA/media/master/2021/30-Nov-2021/Photos/IMG_1042.JPG
  [OK] /Volumes/SP-3/DCIM/VID_0001.MOV (104.8 MB)  -> Master: /Volumes/SP-1/SPSA/media/master/2021/30-Nov-2021/Videos/VID_0001.MOV
  ... (1,578 more items)

Zero files were deleted. Re-run with `--execute` to delete redundant copies.
```
