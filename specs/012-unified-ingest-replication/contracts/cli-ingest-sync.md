# Contract: CLI Commands for Unified Ingestion & Replica Synchronization

**Feature Branch**: `012-unified-ingest-replication`  
**Date**: 2026-11-10  
**Status**: Specification

---

## 1. Overview
This contract defines the command-line interface arguments, exit codes, output formats, and interactive behavior for master ingestion and multi-replica synchronization.

---

## 2. Command: `metadata-extractor ingest`

Transfers assets marked `NeedsIngestToMaster` from a source drive into the primary master drive structure (`<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}`).

### Usage
```bash
metadata-extractor ingest [OPTIONS] --source-drive <SOURCE_DRIVE>
```

### Options
| Option | Short | Type | Default | Description |
|---|---|---|---|---|
| `--source-drive` | `-s` | String | *(Required)* | Source drive identifier, external drive mount path (`/Volumes/USB`), or local directory path (`./incoming`, `~/Downloads/import`). No preconfiguration required. |
| `--target-drive` | `-t` | String | None | Target primary master drive identifier (defaults to configured primary drive in `drives.yaml`) |
| `--config` | `-c` | Path | `drives.yaml` | Path to drive configuration file or synthesizes from `.env` |
| `--replicate` | `-r` | Flag | `false` | **NEW**: Automatically fan out newly ingested canonical masters to all mounted replica drives upon ingestion completion |
| `--dry-run` | | Flag | `false` | Preview planned transfers and collision resolutions without writing files or mutating database records |
| `--delete-source` | | Flag | `false` | Physically delete source files after verified transfer (default: marks source record as `MarkForRemoval` for deferred pruning) |
| `--no-verify-hash` | | Flag | `false` | Skip destination BLAKE3 cryptographic hash verification (file size verification remains enforced) |
| `--format` | | String | `text` | Output format: `text` (human-readable table/summary) or `json` (machine-readable NDJSON) |
| `--quiet` | `-q` | Flag | `false` | Suppress ANSI progress bars and live heartbeats |
| `--json-telemetry` | | Flag | `false` | Emit progress telemetry as discrete JSON lines on stderr |

### Exit Codes
- `0`: Success (all planned files transferred or skipped identical without errors).
- `1`: Configuration error (missing drive definitions or invalid configuration).
- `2`: Hardware error (source or target drive mount path does not exist).
- `3`: Transfer or verification error (one or more files failed transfer or hash verification).

---

## 3. Command: `metadata-extractor sync-replicas`

Fans out `CanonicalMaster` assets from the primary master drive to configured replica drives (`replica1`, `replica2`, ...).

### Aliases
`replicate`, `replica-sync`

### Usage
```bash
metadata-extractor sync-replicas [OPTIONS]
```

### Options
| Option | Short | Type | Default | Description |
|---|---|---|---|---|
| `--replica` | `-r` | String | None | Specific replica drive ID or group to synchronize (defaults to all mounted replica drives if omitted) |
| `--primary` | `-p` | String | None | Override primary master drive ID (defaults to configured primary drive) |
| `--config` | `-c` | Path | `drives.yaml` | Path to drive configuration file or synthesizes from `.env` |
| `--dry-run` | | Flag | `false` | Preview planned replica transfers without copying files or mutating database records |
| `--no-verify-hash` | | Flag | `false` | Skip destination BLAKE3 cryptographic hash verification (byte size verification remains enforced) |
| `--format` | | String | `text` | Output format: `text` or `json` |
| `--quiet` | `-q` | Flag | `false` | Suppress ANSI progress bars and live heartbeats |
| `--json-telemetry` | | Flag | `false` | Emit progress telemetry as discrete JSON lines on stderr |

### Exit Codes
- `0`: Success (all targeted mounted replicas synchronized cleanly).
- `1`: Configuration error.
- `2`: Primary drive unmounted or inaccessible.
- `3`: Transfer or verification failure on one or more replica files.

---

## 4. Output Schemas

### Text Output Example (`ingest --replicate`)
```text
=== Master Ingestion Execution ===
Source Drive:       staging_drive (/Volumes/USB_Drive)
Target Drive:       primary_main (/Volumes/SP-1/SPSA/media/master)
Status:             Completed (Verified)
Files Transferred:  142
Files Skipped:      3 (identical copies already present)
Files Failed:       0
Volume Transferred: 2.84 GB
Throughput:         189.33 MB/s
Elapsed Time:       15.22s

=== Replica Synchronization Execution ===
Target Replicas:    replica_1 (Mounted), replica_2 (Mounted)
Offline Replicas:   replica_3 (Offline - skipped)
Files Replicated:   284 (142 per mounted replica)
Files Skipped:      0
Files Failed:       0
Volume Transferred: 5.68 GB
Throughput:         175.40 MB/s
Elapsed Time:       32.38s

Catalog updated: 142 CanonicalMaster, 284 ReplicaKeep, 142 MarkForRemoval.
```

### JSON Output Example (`--format json`)
```json
{
  "operation": "ingest_and_replicate",
  "is_dry_run": false,
  "ingest": {
    "source_drive_id": "staging_drive",
    "target_drive_id": "primary_main",
    "files_transferred": 142,
    "files_skipped_identical": 3,
    "files_failed": 0,
    "bytes_transferred": 3049281740,
    "source_files_deleted": 0,
    "elapsed_seconds": 15.22,
    "errors": []
  },
  "replication": {
    "primary_drive_id": "primary_main",
    "synchronized_replicas": ["replica_1", "replica_2"],
    "offline_replicas": ["replica_3"],
    "files_replicated": 284,
    "files_skipped_identical": 0,
    "files_failed": 0,
    "bytes_transferred": 6098563480,
    "elapsed_seconds": 32.38,
    "errors": []
  }
}
```
