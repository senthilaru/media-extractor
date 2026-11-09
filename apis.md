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
   - [dedup-stream](#dedup-stream)
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

### Query Exact Duplicates using `jq` (CLI)

```bash
# Find all duplicate files grouped by BLAKE3 cryptographic hash:
jq -s 'group_by(.fingerprint.cryptographic_hash) | map(select(length > 1) | {hash: .[0].fingerprint.cryptographic_hash, count: length, files: map(.file_context.file_path)})' media_catalog.ndjson
```

### Query Exact Duplicates in MongoDB (`mongosh`)

```javascript
db.items.aggregate([
  {
    $group: {
      _id: "$fingerprint.cryptographic_hash",
      count: { $sum: 1 },
      files: { $push: "$file_context.file_path" },
      total_size: { $sum: "$file_context.file_size_bytes" }
    }
  },
  { $match: { count: { $gt: 1 } } },
  { $sort: { count: -1 } }
]);
```

### Query Potential Video Duplicate Clips in MongoDB (`mongosh`)

```javascript
// Find videos with identical duration and capture timestamp (likely same recording)
db.items.aggregate([
  { $match: { "file_context.media_category": "Video", "video.duration_ms": { $ne: null } } },
  {
    $group: {
      _id: {
        duration: "$video.duration_ms",
        captured: "$temporal.iso_timestamp"
      },
      count: { $sum: 1 },
      files: { $push: "$file_context.file_path" }
    }
  },
  { $match: { count: { $gt: 1 } } }
]);
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
| `--channel-capacity` | `-c` | `100` | Channel capacity for batch buffer backpressure. |
| `--include-hidden` | — | `false` | Include hidden files and directories. |
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
| `--include-hidden` | — | `false` | Include hidden files and directories (prefixed with `.`). |
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

Run progressive 4-tier deduplication over a target directory:

```bash
# Run full 4-tier deduplication with human-readable summary:
metadata-extractor dedup /Volumes/SP-1/DCIM --format text

# Run deduplication emitting cluster JSON report:
metadata-extractor dedup /Volumes/SP-1/DCIM --format json > dedup_report.json

# Exact bitwise deduplication only (skipping Tier 4 visual hashing):
metadata-extractor dedup /Volumes/SP-1/DCIM --mode exact --format json

# Custom visual similarity threshold (Hamming distance 0..=64):
metadata-extractor dedup /Volumes/SP-1/DCIM --similarity-threshold 8
```

| Option | Flag | Default | Description |
|---|---|---|---|
| `<PATH>` | *(Positional)* | Required | Path to directory or media file to scan and deduplicate. |
| `--mode` | — | `all` | Deduplication mode: `all` (4-tier), `exact` (bitwise only), or `perceptual` (visual only). |
| `--similarity-threshold` | — | `10` | Maximum Hamming distance threshold ($0..=64$) for visual near-duplicate clustering. |
| `--no-perceptual` | — | `false` | Disable Tier 4 perceptual visual deduplication (exact bitwise only). |
| `--no-rotation` | — | `false` | Disable 90°/180°/270° orientation invariance (0° canonical orientation only). |
| `--format` | — | `json` | Output format: `json` (cluster JSON report) or `text` (human-readable table). |
| `--batch-size` | `-b` | `500` | Discovery batch chunk size when scanning directory. |
| `--channel-capacity` | `-c` | `100` | Channel capacity for batch buffer backpressure. |
| `--include-hidden` | — | `false` | Include hidden files and directories. |
| `--follow-symlinks` | — | `false` | Follow directory symlinks with loop detection. |
| `--quiet` | `-q` | `false` | Suppress live progress messages on `stderr`. |
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
│   ├── mongodb_sink.rs          # Production MongoDB adapter (bulk upserts & batch pre-filter)
│   └── terminal_progress.rs     # Indicatif ANSI terminal progress bar & JSON telemetry
├── application/                 # Application Layer (Use Cases & Orchestration)
│   ├── mod.rs
│   ├── engine.rs                # ScanEngine coordinating traversal and backpressure
│   ├── metadata_engine.rs       # MetadataExtractionEngine coordinating extraction
│   ├── pipeline.rs              # ExtractionPipeline decoupled discovery/worker/persistence
│   ├── progress_tracker.rs      # Atomic progress telemetry & moving-average rate calculator
│   └── dedup_engine.rs          # MediaDeduplicationEngine orchestrating 4-tier pipeline
└── main.rs                      # Unified CLI entry point & subcommands
```

---

## 8. Testing, Benchmarking & Verification

```bash
# Run complete test suite (83 unit and integration tests)
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
