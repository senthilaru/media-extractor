# Media Metadata Extractor — APIs, Schemas & Subcommands Reference

This document contains detailed reference documentation for data schemas, subcommands, Unix streaming pipelines, duplicate identification algorithms, database query recipes, and internal architecture.

For primary setup and the unified command guide, see [README.md](README.md).

---

## Table of Contents

1. [Normalized Document Schema (MongoDB-Ready)](#1-normalized-document-schema-mongodb-ready)
2. [Multi-Tier Fingerprinting & Duplicate Detection Reference](#2-multi-tier-fingerprinting--duplicate-detection-reference)
3. [Duplicate Detection & Query Recipes (jq & MongoDB)](#3-duplicate-detection--query-recipes-jq--mongodb)
4. [Subcommands Reference](#4-subcommands-reference)
   - [extract](#extract)
   - [scan](#scan)
   - [extract-stream](#extract-stream)
   - [dedup](#dedup)
   - [dedup prune](#dedup-prune)
   - [dedup-stream](#dedup-stream)
   - [ingest](#ingest)
   - [Multi-Drive Configuration Format (drives.yaml)](#multi-drive-configuration-format-drivesyaml)
5. [Unix Pipelined Streaming Workflows](#5-unix-pipelined-streaming-workflows)
6. [Direct MongoDB Ingestion & Query API](#6-direct-mongodb-ingestion--query-api)
7. [System Architecture & Rust Library Reference](#7-system-architecture--rust-library-reference)
8. [Testing, Benchmarking & Verification](#8-testing-benchmarking--verification)

---

## 1. Normalized Document Schema (MongoDB-Ready)

Every emitted JSON record strictly complies with document database schemas and pre-computes integer calendar facets (`year`, `month`, `day`, `hour`, `minute`, `second`) for high-speed indexing:

```json
{
  "id": "e6f9a0c2-124b-4b13-9a3b-28f9d0c6418b",
  "file_context": {
    "scanned_root": "/Volumes/SP-1/DCIM/100CANON",
    "file_path": "/Volumes/SP-1/DCIM/100CANON/IMG_1042.CR3",
    "relative_path": "IMG_1042.CR3",
    "file_name": "IMG_1042.CR3",
    "file_size_bytes": 34819200,
    "extension": "cr3",
    "media_category": "RawPhoto",
    "modified_at": "2026-10-14T09:30:15Z"
  },
  "temporal": {
    "timestamp_utc": "2026-10-14T09:30:15Z",
    "iso_timestamp": "2026-10-14T09:30:15+00:00",
    "year": 2026,
    "month": 10,
    "day": 14,
    "hour": 9,
    "minute": 30,
    "second": 15,
    "source": "ExifDateTimeOriginal"
  },
  "camera": {
    "make": "Canon",
    "model": "Canon EOS R5",
    "lens_model": "RF24-70mm F2.8 L IS USM",
    "software": "Firmware 1.8.1"
  },
  "exposure": {
    "iso": 400,
    "f_number": 2.8,
    "exposure_time": "1/500",
    "exposure_time_secs": 0.002,
    "focal_length_mm": 50.0,
    "focal_length_in_35mm_film": 50,
    "exposure_program": "AperturePriority",
    "metering_mode": "MultiSegment",
    "flash": "DidNotFire",
    "white_balance": "Auto"
  },
  "location": {
    "latitude": 37.774929,
    "longitude": -122.419416,
    "altitude_meters": 15.2
  },
  "dimensions": {
    "width": 8192,
    "height": 5464,
    "orientation": 1
  },
  "video": {
    "duration_ms": 12450,
    "display_width": 3840,
    "display_height": 2160,
    "frame_rate_fps": 59.94,
    "video_codec": "h264",
    "audio_codec": "aac",
    "rotation_degrees": 0
  },
  "fingerprint": {
    "id": "7328e480-88bc-4154-b45f-0b62e84af23f",
    "file_path": "/Volumes/SP-1/DCIM/100CANON/IMG_1042.CR3",
    "file_size_bytes": 34819200,
    "media_category": "RawPhoto",
    "sample_digest": "4f8a3b2c1d0e...",
    "cryptographic_hash": "a1b2c3d4e5f67890123456789abcdef0123456789abcdef0123456789abcdef0",
    "phash": 10290360256470180180,
    "phash_hex": "8eceb421f1b3e154",
    "rotated_hashes": {
      "hash_90": 13483331400534102346,
      "hash_180": 15809008581847135438,
      "hash_270": 17200441951722063073
    },
    "is_low_entropy": false,
    "fingerprinted_at": "2026-10-14T09:35:00Z"
  },
  "extraction_status": {
    "status": "Success",
    "warnings": []
  }
}
```

> [!NOTE]
> **BSON 64-Bit Integer Compatibility**: In BSON, 64-bit integers are signed (`Int64`). When storing 64-bit perceptual hashes (`u64`) where the most significant bit is set (`>= 0x8000_0000_0000_0000`), the value is stored in BSON as a two's-complement signed `Int64`. The engine automatically reinterprets signed BSON `Int64` integers back into full 64-bit unsigned `u64` values during deserialization, ensuring seamless interoperability between Rust and MongoDB.

---

## 2. Multi-Tier Fingerprinting & Duplicate Detection Reference

### Hash Calculation Methodologies

| Hash Parameter | Photos & Camera RAW | Videos (MP4, MOV, MKV) | Calculation Method & Resource Characteristics |
|---|---|---|---|
| **`fingerprint.cryptographic_hash`** | Computed | Computed | **Streaming BLAKE3 256-bit digest** read in 64 KB chunks until EOF. Low memory overhead (< 100 MB RAM) even on multi-GB video files. 100% collision-resistant bitwise identity. |
| **`fingerprint.sample_digest`** | Computed | Computed | **BLAKE3 fast sample digest** combining 64-bit file byte length + first 16 KB (MP4 `ftyp`/header) + last 16 KB (MP4 `moov`/trailer). Rapid pre-filter. |
| **`fingerprint.phash`** (or `phash_hex`) | Computed | **Omitted (`null`)** | **64-bit Discrete Cosine Transform (DCT)** visual hash computed on a 32x32 luminance thumbnail. Deliberately omitted for video containers to avoid heavy GOP frame decoding / memory spikes. |
| **`fingerprint.rotated_hashes`** | Computed | **Omitted (`null`)** | 90°, 180°, and 270° orientation variants of `phash` for rotation-invariant visual matching. |
| **`fingerprint.is_low_entropy`** | Computed | `false` | Blank/monochromatic image guard to prevent false-positive grouping of dark/black frames. |

---

### Duplicate Identification Criteria

#### A. Exact Bitwise Duplicates (Photos & Videos)
For files that are 100% identical copies (even if filenames or directory locations differ):
- **Primary Match**: `fingerprint.cryptographic_hash`
  - Group records by `fingerprint.cryptographic_hash`. If two or more records share the exact same hash, they are 100% bitwise duplicate assets.
- **Fast Pre-filter**: `file_context.file_size_bytes` and `fingerprint.sample_digest`
  - If byte sizes or sample digests do not match, files are guaranteed not to be identical.

#### B. Visual & Near-Duplicates (Photos Only)
For photos that were resized (e.g., thumbnails), recompressed (JPEG quality loss), or rotated:
- **Primary Match**: `fingerprint.phash` (or `fingerprint.phash_hex`)
  - Compute the **Hamming distance** (number of differing bits out of 64) between two photos:
    - **Distance = 0**: Visually identical photo (different compression, metadata, or format).
    - **Distance $\le 10$**: Perceptual near-duplicate (downscaled thumbnail, minor crop, or re-encode).
    - **Distance $> 10$**: Distinct visual content.
- **Rotation Matching**: `fingerprint.rotated_hashes` (`hash_90`, `hash_180`, `hash_270`)
  - Compare photo A's `phash` against photo B's rotated hashes to detect duplicates rotated by 90°, 180°, or 270°.
- **Blank Frame Guard**: `fingerprint.is_low_entropy`
  - Always verify `fingerprint.is_low_entropy == false` before grouping; do not cluster blank, black, or low-contrast frames together.

#### C. Near-Duplicates / Re-encoded Copies (Videos Only)
Since video frames are not decoded for `phash`, use container signatures to find identical or re-encoded recordings:
- **Primary Match**: `video.duration_ms` + `temporal.iso_timestamp`
  - If two videos share the exact same playtime duration (within $\pm 100\text{ ms}$) and identical UTC capture timestamp, they represent the same recorded clip.
- **Secondary Match**: `dimensions.width`, `dimensions.height`, and `video.frame_rate_fps`
  - Confirms matching resolution and frame rate across containers.

---

## 3. Duplicate Detection & Query Recipes (jq & MongoDB)

### How Duplicate Marking Works (`dedup -c drives.yaml`)

When you index multiple hard drives into MongoDB, every media file has an exact content fingerprint (`fingerprint.cryptographic_hash` generated via BLAKE3).

To mark duplicates across all connected or previously scanned drives:
1. Define your drive roles and canonical paths in `drives.yaml` (or in `.env`).
2. Run the deduplication evaluation command:
   ```bash
   # Preview classifications without modifying MongoDB (Safe Dry-Run):
   metadata-extractor dedup -c drives.yaml --dry-run

   # Classify duplicates and persist 'dedup' metadata to MongoDB:
   metadata-extractor dedup -c drives.yaml
   ```

When executed, the engine:
1. Groups all documents in MongoDB by exact BLAKE3 `fingerprint.cryptographic_hash`.
2. Evaluates each file against the configured drive hierarchy:
   - If a copy exists in the primary master directory (`<MasterRoot>/<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}`), it is marked **`CanonicalMaster`**.
   - Valid backup copies on configured replica drives are marked **`ReplicaKeep`**.
   - Redundant duplicate copies on secondary drives or non-canonical folders are marked **`MarkForRemoval`** (and their `reclaimable_bytes` calculated).
   - Unique files existing only on a secondary drive are marked **`NeedsIngestToMaster`**.
3. Writes a `dedup` object onto each document in MongoDB using high-throughput bulk write operations.

---

### The `dedup` Embedded Document Schema

After running `dedup -c drives.yaml`, each media document in the MongoDB collection (`media_assets`) contains an embedded `dedup` object:

```json
{
  "_id": ObjectId("654a1b..."),
  "id": "e6f9a0c2-124b-4b13-9a3b-28f9d0c6418b",
  "file_context": {
    "file_path": "/Volumes/SP-2/DCIM/100CANON/IMG_1042.JPG",
    "file_name": "IMG_1042.JPG",
    "file_size_bytes": 14205819
  },
  "fingerprint": {
    "cryptographic_hash": "b2c3d4e5f601728394a5b6c7d8e9f0123456789abcdef0123456789abcdef012"
  },
  "dedup": {
    "cluster_id": "99312b10-0a12-4f3b-9a4c-88201a43b2cf",
    "status": "MarkForRemoval",
    "canonical_master_id": "78201a43-0b12-4c3d-8e4f-0123456789ab",
    "canonical_master_path": "/Volumes/SP-1/SPSA/media/master/2021/30-Nov-2021/Photos/IMG_1042.JPG",
    "drive_id": "replica_1",
    "drive_group": "replica1",
    "is_in_master_path": false,
    "reclaimable_bytes": 14205819,
    "evaluated_at": "2026-11-10T15:20:00Z"
  }
}
```

#### Deduplication Status Reference

| `dedup.status` | Description | Action Required |
|---|---|---|
| `CanonicalMaster` | The definitive master file stored on the primary drive in the canonical folder structure. | **None** (Protected; never deleted). |
| `ReplicaKeep` | Verified mirror copy on a configured replica drive in its master path. | **None** (Retained as backup). |
| `MarkForRemoval` | Redundant duplicate on a secondary drive or outside the master folder. | Run `metadata-extractor dedup prune --drive <ID> -c drives.yaml` to delete. |
| `NeedsIngestToMaster` | Unique asset present only on a secondary/external drive and missing from the master drive. | Run `metadata-extractor ingest --source-drive <PATH_OR_ID> -c drives.yaml` to copy and verify. |
| `ConflictReview` | Multiple differing files found in the canonical master destination requiring review. | Manual inspection. |
| `Removed` | File was physically unlinked from disk by `dedup prune --execute` or `ingest --delete-source`. | Disk space reclaimed. |

---

### MongoDB Query Recipes (`mongosh`)

You can query your indexed media and marked duplicates using the `mongosh` terminal or MongoDB Compass:

```bash
# Connect to your MongoDB instance
mongosh "mongodb://localhost:27017/media_index_db"
```

#### 1. Summary Tally of Assets by Deduplication Status
Get a high-level breakdown of all files, statuses, and storage volumes in gigabytes:

```javascript
db.media_assets.aggregate([
  {
    $group: {
      _id: "$dedup.status",
      count: { $sum: 1 },
      total_gb: { $sum: { $divide: ["$file_context.file_size_bytes", 1073741824] } }
    }
  },
  { $sort: { count: -1 } }
]);
```

#### 2. Find All Files Marked for Removal
List files that have been verified as redundant and can be safely pruned, alongside their canonical master reference:

```javascript
db.media_assets.find(
  { "dedup.status": "MarkForRemoval" },
  {
    "file_context.file_path": 1,
    "file_context.file_size_bytes": 1,
    "dedup.canonical_master_path": 1,
    "dedup.drive_id": 1
  }
).limit(50);
```

#### 3. Total Reclaimable Disk Space Across All Disks
Calculate how many gigabytes will be freed once all marked duplicates are pruned:

```javascript
db.media_assets.aggregate([
  { $match: { "dedup.status": "MarkForRemoval" } },
  {
    $group: {
      _id: null,
      total_files_to_remove: { $sum: 1 },
      total_reclaimable_gb: { $sum: { $divide: ["$dedup.reclaimable_bytes", 1073741824] } }
    }
  }
]);
```

#### 4. Find Marked Duplicates on a Specific External Drive
Filter files ready for pruning on a specific mounted volume (e.g. `/Volumes/SP-2`):

```javascript
db.media_assets.find(
  {
    "dedup.status": "MarkForRemoval",
    "file_context.file_path": { $regex: "^/Volumes/SP-2" }
  },
  {
    "file_context.file_path": 1,
    "dedup.canonical_master_path": 1
  }
);
```

#### 5. Find Files Needing Ingestion into the Primary Master
Find unique photos and videos on secondary drives that have not yet been copied to the primary master:

```javascript
db.media_assets.find(
  { "dedup.status": "NeedsIngestToMaster" },
  {
    "file_context.file_path": 1,
    "file_context.file_size_bytes": 1,
    "file_context.media_category": 1,
    "temporal.iso_timestamp": 1
  }
);
```

#### 6. Inspect Master vs Duplicate Pairs for a Single Hash
Look up all copies of a specific file by its BLAKE3 hash to see where the master is and where duplicates live:

```javascript
db.media_assets.find(
  { "fingerprint.cryptographic_hash": "b2c3d4e5f601728394a5b6c7d8e9f0123456789abcdef0123456789abcdef012" },
  {
    "file_context.file_path": 1,
    "dedup.status": 1,
    "dedup.is_in_master_path": 1,
    "dedup.canonical_master_path": 1
  }
);
```

#### 7. Query Un-Evaluated Exact Duplicates (Raw Hashing Grouping)
If you have not run `dedup -c drives.yaml` yet and want to see raw duplicates grouped purely by cryptographic hash:

```javascript
db.media_assets.aggregate([
  {
    $group: {
      _id: "$fingerprint.cryptographic_hash",
      count: { $sum: 1 },
      files: { $push: "$file_context.file_path" },
      total_size_bytes: { $sum: "$file_context.file_size_bytes" }
    }
  },
  { $match: { count: { $gt: 1 } } },
  { $sort: { count: -1 } }
]);
```

### Query Exact Duplicates using `jq` (CLI)

```bash
# Find all duplicate files grouped by BLAKE3 cryptographic hash from an exported NDJSON file:
jq -s 'group_by(.fingerprint.cryptographic_hash) | map(select(length > 1) | {hash: .[0].fingerprint.cryptographic_hash, count: length, files: map(.file_context.file_path)})' media_catalog.ndjson
```

---

## 4. Subcommands Reference

In addition to the primary unified command (`metadata-extractor <PATH>`), specialized subcommands are available for modular script composition.

### `extract`

Extract rich metadata from a single file or directory:

```bash
# Extract all media files in a directory:
metadata-extractor extract /Volumes/SP-1/DCIM

# Extract a single file:
metadata-extractor extract /Volumes/SP-1/DCIM/IMG_0001.JPG

# Output compact NDJSON:
metadata-extractor extract /Volumes/SP-1/DCIM/IMG_0001.JPG --pretty false
```

| Option | Flag | Default | Description |
|---|---|---|---|
| `<PATH>` | *(Positional)* | Required | Path to directory or media file to scan and extract. |
| `--output` | `-o` | `stdout` | Destination file path to write extracted metadata records. |
| `--pretty` | `-p` | `true` (file) / `false` (dir) | Format output with pretty indentation (`true` or `false`). |
| `--batch-size` | `-b` | `500` | Discovery batch chunk size when scanning directory. |
| `--channel-capacity` | — | `100` | Channel capacity for batch buffer backpressure. |
| `--include-hidden` | — | `true` | Include hidden files and directories. |
| `--follow-symlinks` | — | `false` | Follow directory symlinks with cycle detection. |
| `--quiet` | `-q` | `false` | Suppress live progress messages on `stderr`. |
| `--help` | `-h` | — | Display help information. |

---

### `scan`

High-throughput directory traversal emitting discovered media item batches:

```bash
# Scan a volume:
metadata-extractor scan /Volumes/SP-1

# Scan multiple directories simultaneously:
metadata-extractor scan /Volumes/SP-1 /Volumes/Backup-Drive /Users/username/Pictures

# Emit batch JSON lines:
metadata-extractor scan /Volumes/SP-1 --format json -q
```

| Option | Flag | Default | Description |
|---|---|---|---|
| `<PATHS>...` | *(Positional)* | Required | One or more directory paths or mount points (e.g. `/Volumes/SP-1`). |
| `--batch-size` | `-b` | `500` | Number of discovered media items per batch chunk. |
| `--channel-capacity` | `-c` | `100` | Maximum batches buffered in channel before backpressure activates. |
| `--include-hidden` | — | `true` | Include hidden files and directories (prefixed with `.`). |
| `--follow-symlinks` | — | `false` | Follow directory symbolic links (with circular reference detection). |
| `--format` | — | `text` | Output format: `text` (human-readable) or `json` (batch JSON). |
| `--quiet` | `-q` | `false` | Suppress live telemetry heartbeat progress on `stderr`. |
| `--help` | `-h` | — | Display help information. |

---

### `extract-stream`

Read discovered batches from standard input and emit streaming NDJSON records:

```bash
metadata-extractor scan /Volumes/SP-1 --format json -q | metadata-extractor extract-stream > catalog.ndjson
```

| Option | Flag | Default | Description |
|---|---|---|---|
| `--format` | — | `json` | Output wire format (`json` emits NDJSON lines). |
| `--help` | `-h` | — | Display help information. |

---

### `dedup`

Run progressive 4-tier deduplication over a target directory or evaluate database-wide replica duplicates using drive configurations:

```bash
# 1. Local Directory Mode: Run full 4-tier deduplication with human-readable summary
metadata-extractor dedup /Volumes/SP-1/DCIM --format text

# 2. Local Directory Mode: Emit cluster JSON report
metadata-extractor dedup /Volumes/SP-1/DCIM --format json > dedup_report.json

# 3. Local Directory Mode: Exact bitwise deduplication only (skipping Tier 4 visual hashing)
metadata-extractor dedup /Volumes/SP-1/DCIM --mode exact --format json

# 4. Global Database-Wide Mode: Preview evaluations across all drives (Dry Run)
metadata-extractor dedup --config drives.yaml --dry-run

# 5. Global Database-Wide Mode: Classify duplicate clusters in MongoDB
metadata-extractor dedup --config drives.yaml

# 6. Universal Mode: Classify ALL catalog assets (both unique files and duplicate clusters)
metadata-extractor dedup --all --config drives.yaml
```

| Option | Flag | Default | Description |
|---|---|---|---|
| `<PATH>` | *(Positional)* | Optional | Path to directory or media file to scan and deduplicate (omitted when using `--config`). |
| `--config` | `-c` | `MEDIA_DRIVES_CONFIG` | Path to drive configuration YAML/TOML file for database-driven cross-drive deduplication. |
| `--all` | — | `false` | Perform universal classification covering unique media assets as well as duplicate clusters. |
| `--dry-run` | — | `false` | Evaluate and preview classifications without updating MongoDB. |
| `--mongodb-uri` | `--db-uri` | `MONGODB_URI` | MongoDB connection URI override. |
| `--collection` | `--db-collection` | `MONGODB_COLLECTION` | MongoDB collection name override (`media_assets`). |
| `--mode` | — | `all` | Deduplication mode: `all` (4-tier), `exact` (bitwise only), or `perceptual` (visual only). |
| `--similarity-threshold` | — | `10` | Maximum Hamming distance threshold ($0..=64$) for visual near-duplicate clustering. |
| `--no-perceptual` | — | `false` | Disable Tier 4 perceptual visual deduplication (exact bitwise only). |
| `--no-rotation` | — | `false` | Disable 90°/180°/270° orientation invariance (0° canonical orientation only). |
| `--format` | — | `text` / `json` | Output format: `json` (cluster JSON report) or `text` (human-readable table). |
| `--batch-size` | `-b` | `500` | Discovery batch chunk size when scanning directory. |
| `--channel-capacity` | — | `100` | Channel capacity for batch buffer backpressure. |
| `--include-hidden` | — | `true` | Include hidden files and directories. |
| `--follow-symlinks` | — | `false` | Follow directory symlinks with loop detection. |
| `--quiet` | `-q` | `false` | Suppress live progress messages on `stderr`. |
| `--help` | `-h` | — | Display help information. |

---

### `dedup prune`

Safely remove files marked as `MarkForRemoval` from a specific mounted drive, verifying that the canonical master replica is safe:

```bash
# 1. Preview files that will be deleted from a secondary drive (Safe Dry Run):
metadata-extractor dedup prune --drive wd_14tb_backup --config drives.yaml

# 2. Execute deletion, requiring canonical master drive to be mounted on disk:
metadata-extractor dedup prune --drive wd_14tb_backup --config drives.yaml --execute --require-master-disk

# 3. Execute deletion and export an audit manifest of unlinked files:
metadata-extractor dedup prune --drive wd_14tb_backup --config drives.yaml --execute -o removal_manifest.json
```

| Option | Flag | Default | Description |
|---|---|---|---|
| `--drive` | `-d` | **Required** | Drive ID or mount path to prune (e.g. `wd_14tb_backup` or `/Volumes/SP-2`). |
| `--config` | `-c` | `drives.yaml` | Path to drive configuration YAML/TOML file. |
| `--execute` | — | `false` | Actually delete files on disk (defaults to safe dry-run preview if omitted). |
| `--require-master-disk` | — | `false` | Require canonical master drive to be mounted and verified on disk before deletion. |
| `-o`, `--manifest-out` | — | — | Path to save the removal audit manifest JSON file. |
| `--mongodb-uri` | `--db-uri` | `MONGODB_URI` | MongoDB connection URI override. |
| `--collection` | `--db-collection` | `MONGODB_COLLECTION` | MongoDB collection name override. |
| `--help` | `-h` | — | Display help information. |

---

### `dedup-stream`

Ingest metadata NDJSON from standard input and emit streaming `DuplicateCluster` records:

```bash
cat media_metadata.ndjson | metadata-extractor dedup-stream --similarity-threshold 10 > duplicate_clusters.ndjson
```

| Option | Flag | Default | Description |
|---|---|---|---|
| `--similarity-threshold` | — | `10` | Maximum Hamming distance threshold ($0..=64$) for visual near-duplicate clustering. |
| `--no-perceptual` | — | `false` | Disable Tier 4 perceptual visual deduplication (exact bitwise only). |
| `--no-rotation` | — | `false` | Disable rotation invariance (0° canonical orientation only). |
| `--format` | — | `json` | Output wire format (`json` emits NDJSON cluster records). |
| `--help` | `-h` | — | Display help information. |

---

### `ingest`

Transfer unmastered files marked `NeedsIngestToMaster` from any external drive, local directory path, or secondary drive into the canonical master drive folder hierarchy (`<MasterRoot>/<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}`), with 100% bitwise BLAKE3 verification and automatic collision handling:

> 💡 **Note on `--source-drive`**: The source drive does **not** need to be predetermined or configured anywhere in `drives.yaml`. You can pass any external drive mount path (e.g. `/Volumes/USB_DRIVE`), local directory path (e.g. `~/Downloads/import`, `./incoming`), or configured drive ID/mount path.

```bash
# 1. Preview ingestion plan (Dry-Run preview, no disk writes or DB changes):
metadata-extractor ingest --source-drive /Volumes/USB_DRIVE --config drives.yaml --dry-run

# 2. Ingest unmastered files into primary master (marks source records as MarkForRemoval upon verification):
metadata-extractor ingest --source-drive /Volumes/USB_DRIVE --config drives.yaml

# 3. Ingest unmastered files from a local directory and delete verified source files:
metadata-extractor ingest --source-drive ./incoming --config drives.yaml --delete-source

# 4. Explicit target drive override:
metadata-extractor ingest --source-drive /Volumes/USB_DRIVE --target-drive primary_main --config drives.yaml

# 5. Ingest unmastered files AND automatically fan out new canonical masters to all mounted replica drives:
metadata-extractor ingest --source-drive /Volumes/USB_DRIVE --replicate --config drives.yaml
```

| Option | Flag | Default | Description |
|---|---|---|---|
| `--source-drive` | `-s` | **Required** | Source drive ID, external drive mount path, or local directory path holding unmastered files to ingest (unconfigured external drives and local directory paths supported dynamically). |
| `--target-drive` | `-t` | Primary Master | Target master drive ID or mount path (defaults to highest priority master in config). |
| `--config` | `-c` | `drives.yaml` | Path to drive configuration YAML/TOML file. |
| `--replicate` | `-r` | `false` | Automatically replicate new canonical masters to all mounted replica drives upon ingestion. |
| `--dry-run` | — | `false` | Compile and preview ingestion plan without writing files or mutating database. |
| `--delete-source` | — | `false` | Delete verified source files on the secondary drive immediately after transfer. |
| `--no-verify-hash` | — | `false` | Skip post-copy BLAKE3 cryptographic hash verification (fast, file size only). |
| `--format` | — | `text` | Output format (`text` table or `json` report). |
| `--mongodb-uri` | `--db-uri` | `MONGODB_URI` | MongoDB connection URI override. |
| `--collection` | `--db-collection` | `MONGODB_COLLECTION` | MongoDB collection name override. |
| `--quiet` | `-q` | `false` | Suppress non-essential terminal progress telemetry. |
| `--help` | `-h` | — | Display help information. |

---

### `sync-replicas` (alias: `replicate`)

Synchronize all authoritative `CanonicalMaster` assets from the primary master drive to configured replica backup drives (`replica1`, `replica2`, ...), preserving canonical date/category hierarchies with 100% BLAKE3 hash verification:

```bash
# 1. Preview planned replica transfers across all mounted replica drives (Dry-Run):
metadata-extractor sync-replicas --dry-run --config drives.yaml

# 2. Synchronize all canonical masters to all mounted replica drives:
metadata-extractor sync-replicas --config drives.yaml

# 3. Synchronize canonical masters specifically to a single designated replica:
metadata-extractor sync-replicas --replica replica_1 --config drives.yaml

# 4. JSON-formatted report output:
metadata-extractor sync-replicas --format json --config drives.yaml
```

| Option | Flag | Default | Description |
|---|---|---|---|
| `--replica` | `-r` | All Mounted | Target specific replica drive ID (defaults to all mounted replica drives if omitted). |
| `--primary` | `-p` | Primary Master | Override primary master drive ID (defaults to configured primary group drive). |
| `--config` | `-c` | `drives.yaml` | Path to drive configuration YAML/TOML file or synthesizes from `.env`. |
| `--dry-run` | — | `false` | Preview planned replica transfers without copying files or mutating database. |
| `--no-verify-hash` | — | `false` | Skip destination BLAKE3 cryptographic hash verification (byte size check remains). |
| `--format` | — | `text` | Output format: `text` or `json`. |
| `--mongodb-uri` | `--db-uri` | `MONGODB_URI` | MongoDB connection URI override. |
| `--collection` | `--db-collection` | `MONGODB_COLLECTION` | MongoDB collection name override. |
| `--quiet` | `-q` | `false` | Suppress non-essential terminal progress telemetry. |
| `--help` | `-h` | — | Display help information. |

---

### Multi-Drive Configuration Format (`drives.yaml` & `.env`)

Drive configurations can be supplied either via a structured YAML/TOML file (`drives.yaml`, see `drives.example.yaml`) or configured directly in your `.env` file without needing any YAML file.

#### 1. Direct Configuration via `.env`

You can declare drive mount points directly in `.env`:

```bash
# Primary Master Drive (Priority 100)
DRIVE_PRIMARY_MOUNT=/Volumes/SP-1
DRIVE_PRIMARY_MASTER=/Volumes/SP-1/SPSA/media/master

# Replica Drives (Priority 80, 70, ...)
DRIVE_REPLICA1_MOUNT=/Volumes/SP-2
DRIVE_REPLICA1_MASTER=/Volumes/SP-2/SPSA/media/master
DRIVE_REPLICA2_MOUNT=/Volumes/SP-3
DRIVE_REPLICA2_MASTER=/Volumes/SP-3/SPSA/media/master
```

When no `--config` argument is provided and `drives.yaml` is not present, the system automatically synthesizes a valid drive configuration directly from these environment variables.

#### 2. Template Configuration File (`drives.yaml`)

Copy `drives.example.yaml` to `drives.yaml`. Environment variable interpolation (`${VAR}` and `${VAR:-default}`) is supported:

```yaml
version: "1.0"

# Drive groups with assigned priority (higher number = higher retention priority)
groups:
  primary:
    priority: 100
    description: "Primary master drive group"
  replica1:
    priority: 80
    description: "First replica backup group"
  staging:
    priority: 20
    description: "Temporary ingest / external USB drive"

# Physical or mounted drive instances
# Placeholders like ${DRIVE_PRIMARY_MOUNT:-/Volumes/SP-1} are automatically interpolated from .env
drives:
  - id: "primary_main"
    group: "primary"
    mount_path: "${DRIVE_PRIMARY_MOUNT:-/Volumes/SP-1}"
    master_path: "${DRIVE_PRIMARY_MASTER:-/Volumes/SP-1/SPSA/media/master}"

  - id: "replica_main"
    group: "replica1"
    mount_path: "${DRIVE_REPLICA1_MOUNT:-/Volumes/SP-2}"
    master_path: "${DRIVE_REPLICA1_MASTER:-/Volumes/SP-2/SPSA/media/master}"

  - id: "ext_drive_3"
    group: "staging"
    mount_path: "/Volumes/EXT-DRIVE-3"
```

#### Canonical Master Folder Hierarchy

When files are in `master_path`, they follow a neat temporal and category layout:
```text
/Volumes/SP-1/SPSA/media/master/
  ├── 2021/
  │    └── 30-Nov-2021/
  │         ├── Photos/        # JPG, PNG, HEIC, WEBP
  │         ├── PhotosRaw/     # CR2, CR3, NEF, ARW, DNG
  │         └── Videos/        # MP4, MOV, MKV
  └── 2026/
       └── 14-Oct-2026/
            ├── Photos/
            └── Videos/
```

#### Deduplication Status State Machine

| Status | Meaning |
|---|---|
| `CanonicalMaster` | The definitive original copy on the primary master drive in canonical directory hierarchy. Never deleted. |
| `ReplicaCopy` | Verified replica on a configured replica drive. Retained as valid backup copy. |
| `MarkForRemoval` | Redundant duplicate on a secondary or non-master path. Safe to prune via `dedup prune`. |
| `NeedsIngestToMaster` | Unique asset present only on a secondary drive. Needs to be ingested into master via `ingest`. |
| `Removed` | File was physically unlinked from disk by `dedup prune --execute` or `ingest --delete-source`. |
| `Conflict` | Multiple differing files found in the canonical master location requiring manual inspection. |

---

## 5. Unix Pipelined Streaming Workflows

Compose `scan`, `extract-stream`, and `dedup-stream` into a zero-disk, bounded-RAM Unix pipeline:

```bash
# Stream directory scanner -> metadata extractor -> deduplication engine
metadata-extractor scan /Volumes/SP-1 --format json -q | \
  metadata-extractor extract-stream --format json | \
  metadata-extractor dedup-stream --similarity-threshold 10 > duplicate_clusters.ndjson
```

### Streaming Cluster Schema (`DuplicateCluster`)

```json
{
  "cluster_id": "7328e480-88bc-4154-b45f-0b62e84af23f",
  "relationship_type": "ThumbnailVariant",
  "canonical_asset": {
    "file_path": "/Volumes/SP-1/DCIM/100CANON/IMG_1042.CR3",
    "file_size_bytes": 34819200,
    "dimensions": { "width": 8192, "height": 5464, "orientation": 1 },
    "media_category": "RawPhoto",
    "quality_score": 947.5
  },
  "variants": [
    {
      "file_path": "/Volumes/SP-1/DCIM/100CANON/IMG_1042_thumb.JPG",
      "file_size_bytes": 482100,
      "relationship": "ThumbnailVariant",
      "hamming_distance": 4,
      "similarity_score": 0.938,
      "reclaimable_bytes": 482100
    }
  ],
  "total_reclaimable_bytes": 482100,
  "total_files": 2
}
```

---

## 6. Direct MongoDB Ingestion & Query API

### Bulk Import via `mongoimport`

```bash
mongoimport --uri "mongodb://localhost:27017/media_catalog" \
  --collection items \
  --file media_metadata.ndjson
```

### High-Performance MongoDB Queries

```javascript
// 1. Instant timeline queries using pre-computed integer facets
db.items.find({
  "temporal.year": 2026,
  "temporal.month": 10
});

// 2. Filter by camera hardware
db.items.find({
  "camera.make": "Canon",
  "camera.model": "Canon EOS R5"
});

// 3. Find high-aperture portraits
db.items.find({
  "exposure.f_number": { $lte: 2.0 }
});

// 4. Query files with GPS locations
db.items.find({
  "location": { $ne: null }
});

// 5. Check files with diagnostic warnings
db.items.find({
  "extraction_status.status": "Partial"
});
```

---

## 7. System Architecture & Rust Library Reference

The codebase is organized following **Domain-Driven Design (DDD)** and **Hexagonal Architecture**:

```text
src/
├── lib.rs                       # Public API and convenience re-exports
├── domain/                      # Pure Business Rules (Zero external framework deps)
│   ├── mod.rs
│   ├── models.rs                # DiscoveredMediaItem, DiscoveryBatch, ScanSession, Metrics
│   ├── metadata_models.rs       # MediaMetadataDocument, FileContext, FilterEvaluationResult
│   ├── dedup_models.rs          # MediaFingerprint, DuplicateCluster, CanonicalAsset, Summary
│   ├── ports.rs                 # MetadataStorageSink, ProgressReporter, ProgressSink traits
│   ├── progress_models.rs       # ExecutionSummary, ScanProgressSnapshot, ExtractionPerformanceProfile
│   └── errors.rs                # StorageError, ExtractionError, FingerprintError, ScanError
├── infrastructure/              # Infrastructure Layer (External Adapters)
│   ├── mod.rs
│   ├── walker.rs                # Jwalk/Rayon parallel filesystem walker
│   ├── classifier.rs            # Compile-time zero-allocation phf classifier
│   ├── cycle_detector.rs        # Inode-based circular symlink guard
│   ├── exif_parser.rs           # nom-exif & kamadak-exif photo/RAW extractor
│   ├── video_parser.rs          # Video container header & atom extractor
│   ├── blake3_hasher.rs         # Streaming cryptographic & sample BLAKE3 hasher
│   ├── phash.rs                 # Pure-Rust DCT 64-bit perceptual visual hasher
│   ├── multi_index.rs           # Multi-Index Hashing table (4x16-bit Pigeonhole candidate search)
│   ├── drives_config.rs         # Drive configuration loader, env interpolation & canonical path matcher
│   ├── mongodb_sink.rs          # Production MongoDB adapter (bulk upserts & batch pre-filter)
│   └── terminal_progress.rs     # Indicatif ANSI terminal progress bar & JSON telemetry
├── application/                 # Application Layer (Use Cases & Orchestration)
│   ├── mod.rs
│   ├── engine.rs                # ScanEngine coordinating traversal and backpressure
│   ├── metadata_engine.rs       # MetadataExtractionEngine coordinating extraction
│   ├── pipeline.rs              # ExtractionPipeline decoupled discovery/worker/persistence
│   ├── progress_tracker.rs      # Atomic progress telemetry & moving-average rate calculator
│   ├── drive_pruner.rs          # DrivePruningService for safe deletion of marked duplicates
│   ├── ingest_service.rs        # MasterIngestService for verified ingestion into canonical master
│   └── dedup_engine.rs          # MediaDeduplicationEngine orchestrating 4-tier & database deduplication
└── main.rs                      # Unified CLI entry point & subcommands
```

---

## 8. Testing, Benchmarking & Verification

```bash
# Run complete test suite (99 unit and integration tests)
cargo test

# Run unit tests only
cargo test --test unit

# Run integration tests only
cargo test --test integration

# Run incremental caching tests
cargo test --test test_incremental_filter
cargo test --test test_incremental_integration

# Run performance and progress integration tests
cargo test --test test_progress_performance_integration
cargo test --test test_progress_tracker

# Run Criterion statistical micro-benchmarks
cargo bench --bench dedup_bench -- --test

# Code formatting & strict clippy linter gates
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```
