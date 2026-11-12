# Media Metadata Extractor & Directory Scanner

A high-throughput, asynchronous media discovery and metadata scanning engine built in Rust. Optimized for multi-core CPUs and unified memory architectures (such as Apple Silicon M1/M2/M3/M4), this tool traverses, indexes, and extracts rich EXIF, camera RAW, and video container metadata across millions of photos and videos from Android, iPhone, DSLRs, and storage archives with minimal memory overhead (< 100 MB).

---

## Why Do I Need This?

Managing a personal archive of photos and videos across years of backups is overwhelming:

- **Media Scattered Across 10+ Drives**: Your family photos, mobile backups, and video clips are spread across **10 different external hard drives** of different sizes, USB sticks, and SD cards.
- **Rampant Duplication**: Countless identical copies, renamed files, recompressed variants, and redundant folders are scattered across different drives, eating up terabytes of storage.
- **Self-Hosted Home Media Server**: You want to maintain and curate all your photos and videos on your own **home media server** without relying on third-party cloud storage—but sorting through 10 uncoordinated disks manually is nearly impossible.

### The Solution: A Phased Consolidation Workflow

```text
Drive 1 ──┐
Drive 2 ──┼──> [ metadata-extractor ] ──> Unified MongoDB Media Index
Drive 3 ──┘        (Run One by One)             │
                                                ▼
                                    Global Duplicate Analysis
                                    (Exact Bitwise & Visual Matching)
                                                │
                                                ▼
Re-connect Drives 1-by-1 <────────── Reclaim Terabytes of Disk Space
```

1. **Step 1: Build a Unified Global Index**
   - Connect your external hard drives to your workstation **one by one**.
   - Run `metadata-extractor /Volumes/Drive-X` for each drive, pointing to a shared MongoDB database.
   - The scanner extracts camera EXIF, video atoms, capture dates, and content fingerprints into a single centralized database.
   - If you ever re-scan a drive, **incremental change caching** automatically recognizes previously indexed files and skips them in seconds with zero extra disk I/O.
2. **Step 2: Identify & Mark Duplicates in MongoDB**
   - With your drives indexed in MongoDB, configure your drive roles (primary master vs replicas) in `drives.yaml` or `.env`.
   - Preview the duplicate classification without altering data:
     ```bash
     ./target/release/metadata-extractor dedup -c drives.yaml --dry-run
     ```
   - Mark duplicates directly in MongoDB:
     ```bash
     ./target/release/metadata-extractor dedup -c drives.yaml
     ```
   - This writes a `dedup` status directly onto each asset record in MongoDB (`CanonicalMaster`, `ReplicaKeep`, `MarkForRemoval`, `NeedsIngestToMaster`).
   - Query marked duplicates instantly in MongoDB (`mongosh`):
     ```javascript
     // Find all files marked for removal across all drives
     db.media_assets.find({ "dedup.status": "MarkForRemoval" }, { "file_context.file_path": 1, "dedup.canonical_master_path": 1 });
     ```
3. **Step 3: Deduplicate and Consolidate Cleanly**
   - Connect your external hard drives back one by one to prune marked duplicates, ingest missing files, or synchronize replicas:
     ```bash
     # Ingest unmastered files from any external drive or local folder into the primary master structure:
     # (--source-drive accepts any external drive mount path, local directory, or drive ID; no preconfiguration required)
     ./target/release/metadata-extractor ingest --source-drive /Volumes/USB_STICK -c drives.yaml

     # Ingest AND automatically fan out new canonical masters to all mounted replica drives:
     ./target/release/metadata-extractor ingest --source-drive /Volumes/USB_STICK --replicate -c drives.yaml

     # Mirror all canonical masters from primary master to all connected replica backup drives:
     ./target/release/metadata-extractor sync-replicas -c drives.yaml

     # Prune marked duplicates on a secondary drive to reclaim disk space:
     ./target/release/metadata-extractor dedup prune --drive wd_backup -c drives.yaml --execute
     ```

> 💡 **Looking for API schemas, duplicate detection algorithms, subcommands, or MongoDB query recipes?**  
> See [apis.md](apis.md) for the complete API and architecture reference.

---

## Quick Start

### 1. Build the Binary

```bash
# Ensure Rust 1.82+ is installed
cargo build --release
```

The optimized executable is available at `./target/release/metadata-extractor`.

### 2. Run the Unified Command

The primary interface is a **single unified command with 1 mandatory input path** (either a directory or a single file):

```bash
# 1. Scan a directory with live interactive progress (streams NDJSON to stdout):
./target/release/metadata-extractor /Volumes/SP-1/DCIM

# 2. High-speed fast mode (bypasses full-file bitwise/perceptual hashing for maximum speed):
./target/release/metadata-extractor /Volumes/SP-1/DCIM -f

# 3. Direct ingestion into MongoDB with automatic indexing and idempotent upsert:
export MONGODB_PASSWORD="your_secure_password"
./target/release/metadata-extractor /Volumes/SP-1/DCIM \
  --mongodb-uri "mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db?authSource=media_index_db"

# 4. Save directly to a catalog file:
./target/release/metadata-extractor /Volumes/SP-1/DCIM -o ./media_catalog.ndjson

# 5. Extract a single photo, video, or RAW file (displays pretty JSON):
./target/release/metadata-extractor /Volumes/SP-1/DCIM/100CANON/IMG_1042.CR3
```

---

## Unified Command Usage

```text
metadata-extractor [OPTIONS] <PATH>
```

When given a **directory**, the engine:
1. Concurrently discovers media assets across worker threads with backpressured chunking.
2. Evaluates assets against the database index (when MongoDB is active) to skip unchanged files.
3. Decodes container metadata (EXIF/QuickTime/MP4), dimensions, capture timestamps, and fingerprints.
4. Updates the database via bulk upserts or writes NDJSON records to stdout / file.
5. Displays real-time progress and a final execution summary.

When given a **single file**, it extracts the asset's metadata and outputs structured JSON.

---

## Key Performance Features

### 1. High-Speed Fast Mode (`-f, --fast`)

For large media libraries (e.g. 1 TB with 10,000+ files), computing full-file bitwise BLAKE3 and perceptual visual hashes on every multi-gigabyte video is the primary bottleneck.

Enable fast mode to extract EXIF, QuickTime/MP4 container headers, timestamps, GPS, and dimensions without reading full file bodies:

```bash
# High-speed metadata-only extraction:
./target/release/metadata-extractor /spsa/temp --fast

# Or select an explicit performance mode:
./target/release/metadata-extractor /spsa/temp --mode balanced
```

- **`fast` (`-f`)**: Instant container and EXIF parsing. Bypasses 100% of bitwise and perceptual hashing.
- **`balanced` (default)**: Fast metadata extraction + sample hashing. Files larger than `--max-hash-size` (default: 100 MB) skip full bitwise hashing.
- **`full`**: Complete cryptographic BLAKE3 hashing and perceptual deduplication for all assets regardless of size.

### 2. Incremental Media Scan & Database Change Caching

When re-running a scan against an existing media library (e.g., repeatedly scanning `/spsa/temp` into MongoDB):
- The scanner performs **batch pre-filtering** ($500$ paths per query) over the unique index on `file_context.file_path`.
- If a file's canonical path, byte size (`file_size_bytes`), and modification timestamp (`modified_at`) match existing database records, the file is identified as **unmodified**.
- Unmodified files **completely bypass** worker dispatch, container demuxing, EXIF decoding, and hashing.
- **Repeat scans complete in seconds instead of hours.**

```bash
# Normal re-scan automatically skips unchanged files:
./target/release/metadata-extractor /spsa/temp

# Force full re-extraction of all assets (bypass change cache):
./target/release/metadata-extractor /spsa/temp --force
```

### 3. Real-Time Terminal Telemetry & Summary

Interactive terminal output renders live progress with moving-average throughput and ETA:

```text
[Extraction] ⠋ [===================>       ] 80% | 13,420/13,420 items (Skipped: 13,400 | Extracted: 20) | 1,450.0 items/s | 12.5 MB/s | Elapsed: 2s | ETA: 0s
```

Upon completion, an execution summary box is displayed:

```text
================================================================================
Metadata Extraction Execution Summary
--------------------------------------------------------------------------------
Session ID:          d9b407fe-d0ec-40f4-a0e4-b78807d9b9d3
Target Path:         /spsa/temp
Performance Mode:    Fast (Incremental change caching active)
Total Duration:      2.14s
Items Discovered:    13,420
Items Skipped:       13,400 (Already indexed & unmodified)
Items Extracted:     20 (0.1%)
Data Volume:         1.02 TB (Extracted: 1.2 GB | Cached: 1.018 TB)
Average Throughput:  6,271.0 items/sec (Effective)
Database Ingestion:  15 upserted | 5 modified | 13,400 unchanged
Errors / Warnings:   0 errors, 0 warnings
Status:              Completed successfully
================================================================================
```

---

## Unified Command CLI Options Reference

| Option | Flag | Environment Variable | Default | Description |
|---|---|---|---|---|
| `<PATH>` | *(Positional)* | — | **Required** | Target media directory or single file to scan and extract. |
| `--fast` | `-f` | `MEDIA_FAST_MODE` | `false` | Enable high-speed metadata-only extraction; bypass full-file bitwise and perceptual hashing. |
| `--mode` | — | `MEDIA_EXTRACTION_MODE` | `balanced` | Extraction performance mode: `fast`, `balanced`, or `full`. |
| `--force` | `--reindex` | `MEDIA_FORCE_REINDEX` | `false` | Force complete re-extraction of all assets, bypassing database incremental change caching. |
| `--max-hash-size` | — | `MEDIA_MAX_HASH_SIZE_MB` | `100` | Skip full-file bitwise hashing for files exceeding this size threshold in MB. |
| `--workers` | — | `MEDIA_WORKERS` | CPU cores | Number of concurrent batch extraction workers. |
| `--output` | `-o` | — | `stdout` | Destination file path to write extracted metadata records. |
| `--pretty` | `-p` | — | `false` (dir) / `true` (file) | Format output with pretty indentation (`true` or `false`). |
| `--mongodb-uri` | `--db-uri` | `MONGODB_URI` | — | MongoDB connection URI to persist extracted metadata records. |
| `--collection` | `--db-collection` | `MONGODB_COLLECTION` | `media_assets` | Target collection name within the MongoDB database. |
| `--profile` | — | `APP_PROFILE` | `local` | Operational profile (`local` for development, `production` for strict security checks). |
| `--batch-size` | `-b` | `SCAN_BATCH_SIZE` | `500` | Discovery batch chunk size when scanning directory. |
| `--channel-capacity` | `-c` | `BACKPRESSURE_CAPACITY` | `100` | Maximum batches buffered in channel before backpressure activates. |
| `--include-hidden` | — | — | `true` | Include hidden files and directories (prefixed with `.`). |
| `--follow-symlinks` | — | — | `false` | Follow directory symlinks with cycle detection. |
| `--quiet` | `-q` | — | `false` | Suppress live scan telemetry on `stderr`. |
| `--json-telemetry` | — | `MEDIA_JSON_TELEMETRY` | `false` | Emit progress telemetry as discrete JSON lines instead of ANSI progress bar. |
| `--help` | `-h` | — | — | Display help information. |

---

## Operational Profiles & Environment Configuration

Configuration follows the 12-Factor methodology, loading settings from `.env` files and environment variables.

### Configuration Profiles (`--profile`)

- **`local` (default)**: Optimized for local development. Automatically loads `.env` / `.env.local`, permits local database defaults, and outputs friendly diagnostic messages.
- **`production`**: Hardened for production environments. Enforces non-empty credentials, rejects placeholder secrets (e.g., `<MONGODB_PASSWORD>`), rejects insecure default passwords (`password`, `admin`, `root`), and fails fast ($< 100$ ms) with exit code `2` if mandatory configuration is missing.

### Setting Up `.env`

```bash
# 1. Copy sample configuration
cp .env.example .env

# 2. Configure environment variables in .env:
APP_PROFILE=local
MONGODB_URI=mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db?authSource=media_index_db
MONGODB_PASSWORD=your_secure_password
MONGODB_COLLECTION=media_assets

# (Optional) Direct Media Drives Configuration for Deduplication & Master Ingest
# Copy drives.example.yaml to drives.yaml OR define drive mount paths directly here:
DRIVE_PRIMARY_MOUNT=/Volumes/SP-1
DRIVE_PRIMARY_MASTER=/Volumes/SP-1/SPSA/media/master
DRIVE_REPLICA1_MOUNT=/Volumes/SP-2
DRIVE_REPLICA1_MASTER=/Volumes/SP-2/SPSA/media/master
```

When `.env` is present, you can simply run:

```bash
./target/release/metadata-extractor /Volumes/SP-1/DCIM
```

---

## Supported Media Formats

- **Photos**: JPG, JPEG, PNG, WEBP, GIF, BMP, HEIC, HEIF, AVIF, TIFF, TIF
- **Videos**: MP4, MOV, MKV, AVI, WEBM, M4V, 3GP, WMV, FLV, MTS, M2TS
- **Camera RAW**: CR2, CR3, NEF, ARW, DNG, ORF, RW2, PEF, RAF, SRW

---

## Advanced Documentation

For detailed information, please refer to [apis.md](apis.md):
- [MongoDB-Ready Normalized JSON Schema](apis.md#1-normalized-document-schema-mongodb-ready)
- [Multi-Tier Fingerprinting & Duplicate Detection Reference](apis.md#2-multi-tier-fingerprinting--duplicate-detection-reference)
- [Duplicate Query Recipes with jq and mongosh](apis.md#3-duplicate-detection--query-recipes-jq--mongodb)
- [Subcommands (`scan`, `extract`, `extract-stream`, `dedup`, `dedup-stream`)](apis.md#4-subcommands-reference)
- [Unix Pipelined Streaming Workflows](apis.md#5-unix-pipelined-streaming-workflows)
- [System Architecture & Rust Library Reference](apis.md#7-system-architecture--rust-library-reference)
- [Testing & Verification Guide](apis.md#8-testing-benchmarking--verification)
