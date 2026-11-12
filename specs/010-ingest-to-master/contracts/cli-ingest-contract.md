# Contract: CLI Interface for Ingestion to Primary Master

**Feature**: `010-ingest-to-master`  
**Contract Type**: CLI & Terminal Output Interface  
**Status**: Approved  

---

## 1. CLI Command & Arguments

```bash
metadata-extractor ingest [OPTIONS] --source-drive <DRIVE_ID_OR_MOUNT>
```

| Argument | Flag | Environment Variable | Default | Description |
|---|---|---|---|---|
| `--source-drive` | `-s` | — | **Required** | Source drive ID, external drive mount path, or local directory path holding unmastered files (unconfigured drives supported) |
| `--target-drive` | `-t` | — | First `primary` drive | Target primary master drive ID |
| `--config` | `-c` | `MEDIA_DRIVES_CONFIG` | `drives.yaml` | Path to drive configuration YAML file |
| `--dry-run` | — | — | `false` | Preview planned file transfers without copying or modifying DB |
| `--delete-source` | — | — | `false` | Physically delete source files after verified copy (defaults to deferred `MarkForRemoval`) |
| `--mongodb-uri` | `--db-uri` | `MONGODB_URI` | `mongodb://localhost:27017/media_index_db` | MongoDB connection URI |
| `--collection` | `--db-collection` | `MONGODB_COLLECTION` | `media_assets` | Target MongoDB collection |
| `--format` | — | — | `text` | Output format (`text` or `json`) |
| `--quiet` | `-q` | — | `false` | Suppress non-essential progress telemetry |

---

## 2. Output Reporting Contracts

### A. Dry-Run Planning Output (`--dry-run`)

```text
================================================================================
MASTER INGESTION PLAN (DRY-RUN)
================================================================================
Configuration:         drives.yaml
Source Drive:          seagate_4tb_dump (/Volumes/Dump-1)
Target Primary Master: wd_14tb_primary (/Volumes/SP-1/SPSA/media/master)
Target Free Space:     4.82 TB available (Adequate)
Unmastered Files:      3 files (142.50 MB)
--------------------------------------------------------------------------------
Planned Transfer Operations:
  [Photo]
    From: /Volumes/Dump-1/DCIM/100CANON/IMG_1042.JPG (34.8 MB)
    To:   /Volumes/SP-1/SPSA/media/master/2021/30-Nov-2021/Photos/IMG_1042.JPG
  [RawPhoto]
    From: /Volumes/Dump-1/DCIM/100CANON/IMG_1042.CR3 (42.1 MB)
    To:   /Volumes/SP-1/SPSA/media/master/2021/30-Nov-2021/PhotosRaw/IMG_1042.CR3
  [Video]
    From: /Volumes/Dump-1/Videos/GoPro/GH010023.MP4 (65.6 MB)
    To:   /Volumes/SP-1/SPSA/media/master/2021/30-Nov-2021/Videos/GH010023.MP4
--------------------------------------------------------------------------------
Zero bytes transferred. Re-run without `--dry-run` to execute ingestion.
================================================================================
```

### B. Ingest Execution Output

```text
================================================================================
MASTER INGESTION SUMMARY
================================================================================
Source Drive:          seagate_4tb_dump (/Volumes/Dump-1)
Target Primary Master: wd_14tb_primary (/Volumes/SP-1/SPSA/media/master)
Status:                SUCCESS (100% Verified)
Files Transferred:     3
Files Skipped:         0 (Identical already in master)
Files Failed:          0
Volume Ingested:       142.50 MB
Elapsed Time:          0.482s (295.6 MB/s)
--------------------------------------------------------------------------------
Database Updates:
  - Canonical Masters Created: 3 (Registered on wd_14tb_primary)
  - Source Files Marked:       3 (Updated to 'MarkForRemoval' on seagate_4tb_dump)
Source Files on Disk:  Preserved on source drive. Run `dedup prune --drive seagate_4tb_dump` to delete.
================================================================================
```

### C. Ingest with Immediate Deletion Output (`--delete-source`)

```text
================================================================================
MASTER INGESTION & PRUNE SUMMARY
================================================================================
Source Drive:          seagate_4tb_dump (/Volumes/Dump-1)
Target Primary Master: wd_14tb_primary (/Volumes/SP-1/SPSA/media/master)
Status:                SUCCESS (Transferred & Source Cleaned)
Files Transferred:     3 (142.50 MB)
Source Files Deleted:  3 (142.50 MB reclaimed on seagate_4tb_dump)
Elapsed Time:          0.512s
Database Updates:
  - Canonical Masters Created: 3 ('CanonicalMaster')
  - Source Records Updated:    3 ('Removed')
================================================================================
```

---

## 3. Exit Codes

| Code | Meaning |
|---|---|
| `0` | Ingest completed successfully with 0 errors |
| `1` | Argument validation or configuration error (e.g. drive not configured) |
| `2` | Drive not mounted, inaccessible path, or insufficient target disk space |
| `3` | MongoDB connection or persistence failure |
| `4` | File transfer or integrity verification failed on one or more items |
