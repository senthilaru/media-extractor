# Media Metadata Extractor & Directory Scanner

A high-throughput, asynchronous media discovery and metadata scanning engine built in Rust. Optimized for Apple Silicon (MacBook M1/M2/M3) unified memory and multi-core CPU architecture, this tool is designed to traverse, index, and extract rich EXIF and container metadata across millions of photos and videos from Android, iPhone, DSLRs, and other devices on local drives or mounted external hard disks (such as `/Volumes/SP-1`, SD cards, and USB archives) with minimal memory overhead (< 100 MB).

---

## Key Features

- **High-Throughput Directory Traversal**: Multi-threaded work-stealing filesystem walking (`jwalk` + `rayon`) tuned for Apple Silicon M1 (balancing Performance and Efficiency cores) without thread thrashing.
- **Rich Photo & Camera RAW Metadata Extraction**: Pure-Rust zero-copy parser (`nom-exif` + `kamadak-exif`) extracting camera body/lens, exposure parameters (aperture, shutter speed, ISO, focal length), GPS coordinates in decimal degrees, pixel dimensions, and orientation without decompressing full image pixels.
- **Video Container Header Parsing**: Inspects container atoms for MP4, MOV, MKV, WebM, and 3GP files, extracting duration in milliseconds, display resolution, frame rate, codecs, and container rotation angle.
- **Temporal Normalization & Calendar Faceting**: Decomposes capture timestamps into UTC and query-optimized integer facets (`year`, `month`, `day`, `hour`, `minute`, `second`) for direct MongoDB partitioning and timeline indexing, with automatic filesystem fallback when embedded tags are stripped.
- **Backpressured Streaming Pipeline**: Streams discovered items and normalized metadata documents over asynchronous channels (`tokio::sync::mpsc`) and Rayon work-stealing worker threads, maintaining strictly bounded memory (< 100 MB).
- **4-Tier Perceptual Deduplication & Fingerprinting**:
  - **Tier 1 (Size & Type Pruning)**: Instant zero-IO candidate filtering by file byte size and media category.
  - **Tier 2 (Fast Sample Digest)**: Rapid BLAKE3 digest of file header (first 16 KB), trailer (last 16 KB), and byte length to prune 90%+ of same-size non-identical files.
  - **Tier 3 (Streaming Cryptographic Digest)**: Streaming chunked 64 KB BLAKE3 digest for 100% bitwise identical asset identification with zero full-file memory buffering.
  - **Tier 4 (64-Bit DCT Perceptual Hashing & Multi-Index Hashing)**: Pure-Rust Discrete Cosine Transform visual hashing with automatic 90°/180°/270° rotation invariance, low-entropy blank image guard, and sub-linear candidate search via 4x16-bit Pigeonhole Multi-Index Hashing.
- **Deterministic Canonical Master Selection**: Automatically designates the highest-quality primary master in each cluster (ranking RAW > Photo > Video, higher resolution, larger file size, earlier timestamp) and computes reclaimable storage bytes.
- **Extensive Media Format Support**:
  - **Photos**: JPG, JPEG, PNG, WEBP, GIF, BMP, HEIC, HEIF, AVIF, TIFF, TIF.
  - **Videos**: MP4, MOV, MKV, AVI, WEBM, M4V, 3GP, WMV, FLV, MTS, M2TS.
  - **Camera RAW**: CR2, CR3, NEF, ARW, DNG, ORF, RW2, PEF, RAF, SRW.
- **MongoDB-Ready Normalized JSON Output**: Emits structured, self-contained JSON records (or streaming NDJSON) ready for document database insertion without custom schema transformation.
- **Hexagonal Architecture (DDD)**: Pure domain entities and traits decoupled from infrastructure decoders and CLI orchestrators.

---

## Quick Start (3 Steps)

### Step 1: Ensure Rust is Available
Make sure Rust 1.82+ is installed and your shell environment is loaded:

```bash
# Load Rust environment into current shell
source "$HOME/.cargo/env"

# Verify compiler
rustc --version
cargo --version
```

### Step 2: Build the Optimized Binary
Compile the project in release mode:

```bash
cargo build --release
```

The optimized binary is saved at `./target/release/metadata-extractor`.

### Step 3: Run the Program

The primary interface is a **single unified command with 1 mandatory input path**:

```bash
# Unified recursive directory scan & extraction (streams NDJSON to stdout)
./target/release/metadata-extractor /Volumes/SP-1/DCIM

# Stream directly into MongoDB with automatic indexing and idempotent upsert:
export MONGODB_PASSWORD="your_secure_password"
./target/release/metadata-extractor /Volumes/SP-1/DCIM \
  --mongodb-uri "mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db?authSource=media_index_db"

# Or configure via .env and specify an operational profile:
./target/release/metadata-extractor /Volumes/SP-1/DCIM --profile local

# Save directly to a catalog file with -o / --output
./target/release/metadata-extractor /Volumes/SP-1/DCIM -o ./media_catalog.ndjson

# Single file extraction (displays formatted metadata JSON)
./target/release/metadata-extractor /Volumes/SP-1/DCIM/100CANON/IMG_1042.CR3
```

---

## How to Run: Detailed Usage & Examples

### 1. Unified Media Scan & Metadata Extraction (`metadata-extractor <PATH>`)

Run the scanner with **1 mandatory input path** (either a directory or a single file):
- When given a **directory**, it recursively traverses all nested subdirectories, discovers photos, videos, and camera RAW media files, computes rich EXIF/video metadata and embedded content fingerprints (BLAKE3 + DCT pHash), and streams MongoDB-ready NDJSON records to stdout (or directly to a file via `-o, --output`).
- When given a **single file**, it extracts that asset's rich metadata and fingerprints, displaying it as pretty-printed JSON (or compact NDJSON if redirected or written to file).

```bash
# Scan all nested directories and stream NDJSON to stdout:
./target/release/metadata-extractor /Volumes/SP-1/DCIM

# Stream directly into MongoDB (creates indexes, performs idempotent upserts):
./target/release/metadata-extractor /Volumes/SP-1/DCIM \
  --mongodb-uri "mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db?authSource=media_index_db" \
  --collection media_assets

# Or via cargo:
cargo run --release -- /Volumes/SP-1/DCIM

# Write output directly to a file (bypasses terminal, suitable for background ingestion):
./target/release/metadata-extractor /Volumes/SP-1/DCIM -o ./media_catalog.ndjson

# Mute live telemetry on stderr for quiet batch scripts:
./target/release/metadata-extractor /Volumes/SP-1/DCIM -o ./media_catalog.ndjson -q

# Extract a single photo or camera RAW file:
./target/release/metadata-extractor /Volumes/SP-1/DCIM/100CANON/IMG_1042.CR3
```

**Emitted Document Structure (MongoDB Ready):**
Every emitted JSON record contains complete file context, normalized temporal calendar facets, camera/exposure/video metadata, and multi-tier content fingerprints:

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
    "media_category": "RawPhoto"
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

### Identifying Duplicate Photos & Videos from Extracted Metadata

When scanning your media library, the output JSON contains multi-tier fingerprint fields. Below is a reference for how hashes are computed and which parameters to inspect to identify duplicates.

#### 1. How Hashes are Calculated

| Hash Parameter | Photos & Camera RAW | Videos (MP4, MOV, MKV) | Calculation Method & Resource Characteristics |
|---|---|---|---|
| **`fingerprint.cryptographic_hash`** | Computed | Computed | **Streaming BLAKE3 256-bit digest** read in 64 KB chunks until EOF. Low memory overhead (< 100 MB RAM) even on multi-GB video files. 100% collision-resistant bitwise identity. |
| **`fingerprint.sample_digest`** | Computed | Computed | **BLAKE3 fast sample digest** combining 64-bit file byte length + first 16 KB (MP4 `ftyp`/header) + last 16 KB (MP4 `moov`/trailer). Rapid pre-filter. |
| **`fingerprint.phash`** (or `phash_hex`) | Computed | **Omitted (`null`)** | **64-bit Discrete Cosine Transform (DCT)** visual hash computed on a 32x32 luminance thumbnail. Deliberately omitted for video containers to avoid heavy GOP frame decoding / memory spikes. |
| **`fingerprint.rotated_hashes`** | Computed | **Omitted (`null`)** | 90°, 180°, and 270° orientation variants of `phash` for rotation-invariant visual matching. |
| **`fingerprint.is_low_entropy`** | Computed | `false` | Blank/monochromatic image guard to prevent false-positive grouping of dark/black frames. |

---

#### 2. Parameters to Check to Identify Duplicates

##### A. Exact Bitwise Duplicates (Photos & Videos)
For files that are 100% identical copies (even if filenames or directory locations differ):
- **Primary Match**: `fingerprint.cryptographic_hash`
  - Group records by `fingerprint.cryptographic_hash`. If two or more records share the exact same hash, they are 100% bitwise duplicate assets.
- **Fast Pre-filter**: `file_context.file_size_bytes` and `fingerprint.sample_digest`
  - If byte sizes or sample digests do not match, files are guaranteed not to be identical.

##### B. Visual & Near-Duplicates (Photos Only)
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

##### C. Near-Duplicates / Re-encoded Copies (Videos Only)
Since video frames are not decoded for `phash`, use container signatures to find identical or re-encoded recordings:
- **Primary Match**: `video.duration_ms` + `temporal.iso_timestamp`
  - If two videos share the exact same playtime duration (within $\pm 100\text{ ms}$) and identical UTC capture timestamp, they represent the same recorded clip.
- **Secondary Match**: `dimensions.width`, `dimensions.height`, and `video.frame_rate_fps`
  - Confirms matching resolution and frame rate across containers.

---

#### 3. Ready-to-Use Duplicate Detection Queries

##### Query Exact Duplicates using `jq` (CLI)
```bash
# Find all duplicate files grouped by BLAKE3 cryptographic hash:
jq -s 'group_by(.fingerprint.cryptographic_hash) | map(select(length > 1) | {hash: .[0].fingerprint.cryptographic_hash, count: length, files: map(.file_context.file_path)})' media_catalog.ndjson
```

##### Query Exact Duplicates in MongoDB (`mongosh`)
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

##### Query Potential Video Duplicate Clips in MongoDB (`mongosh`)
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

### 2. Backwards-Compatible Subcommands (`extract`)

The `extract` command accepts **either a directory path or a single file path**:
- When given a **directory**, it utilizes the high-throughput media directory walker to discover all media files recursively, extracts metadata across CPU worker threads in parallel, and streams MongoDB-ready NDJSON records to stdout (with real-time progress on stderr).
- When given a **single file**, it extracts and displays that file's metadata as pretty-printed JSON.

```bash
# Extract all media files in a directory (scans recursively using media walker):
./target/release/metadata-extractor extract /Volumes/SP-1/SPSA/samples/

# Or via cargo:
cargo run --release -- extract /Volumes/SP-1/SPSA/samples/

# Extract a single photo or camera RAW file:
cargo run --release -- extract /Volumes/SP-1/DCIM/100CANON/IMG_1042.CR3

# Compact (single-line) JSON for a single file:
./target/release/metadata-extractor extract /Volumes/SP-1/DCIM/100CANON/IMG_1042.CR3 --pretty false
```

**Sample Output:**
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
    "media_category": "RawPhoto"
  },
  "temporal": {
    "iso_timestamp": "2026-10-14T08:12:45+00:00",
    "year": 2026,
    "month": 10,
    "day": 14,
    "hour": 8,
    "minute": 12,
    "second": 45,
    "source": "ExifDateTimeOriginal"
  },
  "camera": {
    "make": "Canon",
    "model": "Canon EOS R5",
    "lens_model": "RF24-70mm F2.8 L IS USM",
    "software": "Firmware 1.8.1"
  },
  "exposure": {
    "focal_length_mm": 50.0,
    "focal_length_35mm": 50.0,
    "f_number": 2.8,
    "exposure_time_seconds": 0.004,
    "exposure_time_str": "1/250",
    "iso": 100,
    "flash_fired": false
  },
  "location": {
    "latitude": 37.774929,
    "longitude": -122.419416,
    "altitude_meters": 14.5
  },
  "dimensions": {
    "width": 8192,
    "height": 5464,
    "orientation": 1
  },
  "fingerprint": {
    "cryptographic_hash": "4cbcae9bdd7f7e3cc15b06efc06cb0f452b0421c9c6b4ef49bb9948f1ea3db80",
    "sample_digest": "70ccccf275421e7aa261ec3f0d6a53f68fe7d2c9edc79c207ad94f1f7319e0fa",
    "phash": 10290360256470180180,
    "phash_hex": "8eceb421f1b3e154",
    "rotated_hashes": {
      "hash_90": 13483331400534102346,
      "hash_180": 15809008581847135438,
      "hash_270": 17200441951722063073
    },
    "is_low_entropy": false
  },
  "extraction_status": {
    "status": "Success",
    "warnings": []
  }
}
```

---

### 2. High-Throughput Directory Scanning (`scan`)

Recursively traverse external disks (e.g. `/Volumes/SP-1`) or local photo archives. Traversal uses Rayon thread pools and emits real-time telemetry on `stderr`.

```bash
# Scan an external hard drive with human-readable output
./target/release/metadata-extractor scan /Volumes/SP-1

# Scan multiple directories simultaneously
./target/release/metadata-extractor scan /Volumes/SP-1 /Volumes/Backup-Drive /Users/username/Pictures
```

**Real-Time Console Progress:**
```text
[Telemetry] Visited: 1240 | Found: 62100 | Rate: 16840.0 fps | Errors: 0
[Photo] /Volumes/SP-1/DCIM/100CANON/IMG_0001.JPG (3481920 bytes)
[RawPhoto] /Volumes/SP-1/DCIM/100CANON/IMG_0001.CR3 (34819200 bytes)
[Video] /Volumes/SP-1/DCIM/100CANON/VID_0001.MOV (104857600 bytes)

Scan completed in 3.69s. Total batches: 125, Total media files: 62100. Session: dfe6ae23-5bae-49ca-82ec-eded82d869d1
```

**Tuning Performance & Memory:**
```bash
# Adjust batch chunk size and channel buffer capacity
./target/release/metadata-extractor scan /Volumes/SP-1 \
  --batch-size 1000 \
  --channel-capacity 50 \
  --include-hidden \
  --follow-symlinks
```

- `-b, --batch-size 1000`: Emits 1,000 files per batch.
- `-c, --channel-capacity 50`: Buffers up to 50 batches in memory. Traversal pauses automatically if downstream processing is slow (backpressure).
- `--include-hidden`: Scans hidden dot-files and directories (`.hidden`).
- `--follow-symlinks`: Follows directory symbolic links with inode-based loop protection.
- `-q, --quiet`: Mutes live telemetry heartbeats on `stderr`.

---

### 3. End-to-End Pipelined Streaming (`scan | extract-stream`)

Connect directory discovery directly to the metadata extraction engine using Unix pipes. This processes discovered batches in parallel across CPU worker threads and emits streaming newline-delimited JSON (NDJSON) to a file:

```bash
# Pipe discovered batches directly into metadata extraction
./target/release/metadata-extractor scan /Volumes/SP-1 --format json -q | \
  ./target/release/metadata-extractor extract-stream > media_metadata.ndjson
```

**What happens under the hood:**
1. `scan` crawls `/Volumes/SP-1` at $>15,000$ files/sec with zero allocations on extension checks.
2. Batches stream via standard input into `extract-stream`.
3. `extract-stream` dispatches batches across Rayon worker threads, parsing EXIF and video container atoms without loading full image pixels or video frame rasters into RAM.
4. Total memory usage remains strictly bounded under 100 MB.

---

### 4. Deduplicate Media Assets (`dedup`)

The `dedup` command runs the progressive 4-tier deduplication engine across single directories or whole volumes:
- **Tier 1**: Groups files by exact file size. Non-matching files are pruned immediately with zero I/O.
- **Tier 2**: Computes BLAKE3 fast sample digests (first 16 KB + last 16 KB + size) to weed out differing files sharing the same length.
- **Tier 3**: Computes streaming cryptographic BLAKE3 hashes in 64 KB buffers, identifying 100% bitwise duplicate assets.
- **Tier 4**: Computes pure-Rust 64-bit DCT perceptual hashes (pHash) on 32x32 luminance thumbnails, matching near-duplicates (Hamming distance $\le 10$) across 90°, 180°, and 270° rotations with a low-entropy guard preventing false-positive blank/black frame grouping.

```bash
# Run full 4-tier deduplication with human-readable summary:
./target/release/metadata-extractor dedup /Volumes/SP-1/DCIM --format text

# Run deduplication emitting MongoDB-ready JSON report:
./target/release/metadata-extractor dedup /Volumes/SP-1/DCIM --format json > dedup_report.json

# Exact bitwise deduplication only (skipping Tier 4 visual hashing):
./target/release/metadata-extractor dedup /Volumes/SP-1/DCIM --mode exact --format json

# Customize near-duplicate visual similarity threshold (Hamming distance 0..=64):
./target/release/metadata-extractor dedup /Volumes/SP-1/DCIM --similarity-threshold 8
```

**Human-Readable Text Output Example:**
```text
================================================================================
MEDIA DEDUPLICATION REPORT
================================================================================
Total Files Scanned:       11
Unique Media Assets:       8
Duplicate Clusters:        3
Exact Bitwise Duplicates:  2
Perceptual Near-Duplicates:1
Total Reclaimable Space:   2860 bytes (0.00 MB)
Tiers Pruned:              63.0%
Elapsed Time:              0.001s
================================================================================

Cluster #1 [ExactBitwise] (2 files, 1443 bytes reclaimable):
  [Master]  tests/fixtures/dedup_samples/photo_original.jpg (1443 bytes, score: 200.0)
  [Variant] tests/fixtures/dedup_samples/photo_exact_copy.jpg (1443 bytes, rel: ExactBitwise, sim: 1.000, dist: Some(0))

Cluster #2 [ExactBitwise] (2 files, 643 bytes reclaimable):
  [Master]  tests/fixtures/dedup_samples/video_a.mp4 (643 bytes, score: 100.0)
  [Variant] tests/fixtures/dedup_samples/video_a_copy.mp4 (643 bytes, rel: ExactBitwise, sim: 1.000, dist: None)

Cluster #3 [RecompressedVariant] (2 files, 774 bytes reclaimable):
  [Master]  tests/fixtures/dedup_samples/photo_original.jpg (1443 bytes, score: 200.0)
  [Variant] tests/fixtures/dedup_samples/photo_thumbnail.jpg (774 bytes, rel: RecompressedVariant, sim: 0.906, dist: Some(6))
```

---

### 5. Unified Unix Streaming Pipeline (`scan | extract-stream | dedup-stream`)

You can compose `scan`, `extract-stream`, and `dedup-stream` into a zero-disk, bounded-RAM Unix pipeline. Metadata is extracted on the fly and fed directly into the multi-index deduplicator, emitting streaming `DuplicateCluster` records in newline-delimited JSON (NDJSON) to standard output:

```bash
# Stream directory scanner -> metadata extractor -> deduplication engine
./target/release/metadata-extractor scan /Volumes/SP-1 --format json -q | \
  ./target/release/metadata-extractor extract-stream --format json | \
  ./target/release/metadata-extractor dedup-stream --similarity-threshold 10 > duplicate_clusters.ndjson
```

**Streaming NDJSON Cluster Record Schema:**
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

### 6. Direct Ingestion into MongoDB

Because the output format strictly complies with document database schemas and pre-computes integer calendar facets (`year`, `month`, `day`), you can import the generated NDJSON directly into MongoDB with zero transformation:

```bash
# Bulk import into MongoDB
mongoimport --uri "mongodb://localhost:27017/media_catalog" \
  --collection items \
  --file media_metadata.ndjson
```

**High-Performance MongoDB Query Examples:**

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

## CLI Command & Options Reference

### Primary Invocation (Unified Media Scanner)
```text
metadata-extractor [OPTIONS] <PATH>
```

| Option | Flag | Default | Description |
|---|---|---|---|
| `<PATH>` | *(Positional)* | Mandatory | Directory or file path to recursively scan and extract rich metadata |
| `--profile` | — | `local` (or `APP_PROFILE`) | Operational profile (`local` for development defaults, `production` for strict security checks) |
| `--mongodb-uri` | `--db-uri` | (or `MONGODB_URI`) | MongoDB connection URI to persist extracted metadata records |
| `--collection` | `--db-collection` | `media_assets` (or `MONGODB_COLLECTION`) | Target collection name within the MongoDB database |
| `--output` | `-o` | `stdout` | Destination file path to write extracted metadata records |
| `--pretty` | `-p` | `false` (dir) / `true` (file) | Format output with pretty indentation |
| `--batch-size` | `-b` | `500` | Discovery batch chunk size when scanning directory |
| `--channel-capacity` | `-c` | `100` | Maximum batches buffered in channel before backpressure activates |
| `--include-hidden` | — | `false` | Include hidden files and directories (prefixed with `.`) |
| `--follow-symlinks` | — | `false` | Follow directory symlinks with cycle detection |
| `--quiet` | `-q` | `false` | Suppress live scan telemetry heartbeats on `stderr` |
| `--help` | `-h` | — | Display help information |

---

### Operational Profiles & Secret Externalization

The application supports profile-based configuration and externalized secrets per the 12-Factor methodology:

#### 1. Configuration Profiles (`--profile`)
- **`local` (default)**: Optimized for local development. Loads `.env` / `.env.local` automatically, permits local database defaults, and outputs friendly diagnostic messages.
- **`production`**: Hardened for production environments. Enforces non-empty credentials, rejects default or placeholder secrets (e.g., `password`, `admin`, `<MONGODB_PASSWORD>`), and fails fast ($< 100$ms) with exit code `2` if mandatory configuration is missing.

#### 2. Externalizing Secrets via Environment Variables
Credentials and secrets are externalized so they are never hardcoded in commands or files:

```bash
# 1. Copy sample configuration
cp .env.example .env

# 2. Set credentials in environment or .env:
export APP_PROFILE="local"
export MONGODB_URI="mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db?authSource=media_index_db"
export MONGODB_PASSWORD="your_secure_password"

# 3. Run without passing passwords on the CLI:
metadata-extractor /Volumes/SP-1/DCIM
```

**Configuration Precedence**:
1. CLI Arguments (`--profile`, `--mongodb-uri`, `--collection`)
2. Environment Variables (`APP_PROFILE`, `MONGODB_URI`, `MONGODB_PASSWORD`, `MONGODB_COLLECTION`)
3. Environment Files (`.env.{profile}`, `.env` via `dotenvy`)
4. Built-in Defaults (Profile defaults)

---

### Subcommands (Backwards Compatible)

```text
metadata-extractor <COMMAND> [OPTIONS]
```

| Command | Description |
|---|---|
| `scan <PATHS>...` | Recursively scan one or more directories for media assets |
| `extract <PATH>` | Extract rich metadata from a single media file |
| `extract-stream` | Read discovered batches from standard input and stream NDJSON to stdout |
| `dedup <PATH>` | Run progressive 4-tier media deduplication and emit JSON/text report |
| `dedup-stream` | Ingest metadata NDJSON from standard input and emit streaming `DuplicateCluster` records |

---

### `scan` Options

```text
metadata-extractor scan [OPTIONS] <PATHS>...
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

### `extract` Options

```text
metadata-extractor extract [OPTIONS] <PATH>
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

### `extract-stream` Options

```text
metadata-extractor extract-stream [OPTIONS]
```

| Option | Flag | Default | Description |
|---|---|---|---|
| `--format` | — | `json` | Output wire format (`json` emits NDJSON lines). |
| `--help` | `-h` | — | Display help information. |

---

---

### `dedup` Options

```text
metadata-extractor dedup [OPTIONS] <PATH>
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

### `dedup-stream` Options

```text
metadata-extractor dedup-stream [OPTIONS]
```

| Option | Flag | Default | Description |
|---|---|---|---|
| `--similarity-threshold` | — | `10` | Maximum Hamming distance threshold ($0..=64$) for visual near-duplicate clustering. |
| `--no-perceptual` | — | `false` | Disable Tier 4 perceptual visual deduplication (exact bitwise only). |
| `--no-rotation` | — | `false` | Disable rotation invariance (0° canonical orientation only). |
| `--format` | — | `json` | Output wire format (`json` emits NDJSON cluster records). |
| `--help` | `-h` | — | Display help information. |

---

## Verification & Testing

Run all unit and integration tests, benchmarks, formatting, and linter gates:

```bash
# Run complete test suite (52 tests)
cargo test

# Run unit tests only (40 tests)
cargo test --test unit

# Run integration tests only (12 tests)
cargo test --test integration

# Run Criterion statistical micro-benchmarks
cargo bench --bench dedup_bench -- --test

# Verify code formatting & strict clippy linter
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

---

## Architecture & Project Structure

The project follows **Domain-Driven Design (DDD)** and **Hexagonal Architecture**:

```text
src/
├── lib.rs                       # Crate root and public exports
├── domain/                      # Pure Business Rules (Zero external framework deps)
│   ├── mod.rs
│   ├── models.rs                # DiscoveredMediaItem, DiscoveryBatch, ScanSession, Metrics
│   ├── metadata_models.rs       # MediaMetadataDocument, TemporalMetadata, CameraMetadata
│   ├── dedup_models.rs          # MediaFingerprint, DuplicateCluster, CanonicalAsset, Summary
│   ├── ports.rs                 # DirectoryScanner, MediaMetadataExtractor, DeduplicationEngine traits
│   └── errors.rs                # ScanError, ExtractionError, FingerprintError, DeduplicationError
├── infrastructure/              # Infrastructure Layer (External Adapters)
│   ├── mod.rs
│   ├── walker.rs                # Jwalk/Rayon parallel filesystem walker
│   ├── classifier.rs            # Compile-time zero-allocation phf classifier
│   ├── cycle_detector.rs        # Inode-based circular symlink guard (dev, ino)
│   ├── telemetry.rs             # Atomic session metrics and heartbeat emitter
│   ├── exif_parser.rs           # nom-exif & kamadak-exif photo/RAW extractor
│   ├── video_parser.rs          # Video container header & atom extractor
│   ├── blake3_hasher.rs         # BLAKE3 fast sample digest and streaming cryptographic hasher
│   ├── phash.rs                 # Pure-Rust Discrete Cosine Transform (DCT) 64-bit perceptual visual hasher
│   ├── raster_decoder.rs        # Bounded image/RAW decoder resizing directly to 32x32 grayscale
│   └── multi_index.rs           # Multi-Index Hashing table (4x16-bit Pigeonhole candidate search)
├── application/                 # Application Layer (Use Cases & Orchestration)
│   ├── mod.rs
│   ├── batch_accumulator.rs     # Bounded batch chunking and channel emission
│   ├── engine.rs                # ScanEngine coordinating traversal and backpressure
│   ├── metadata_engine.rs       # MetadataExtractionEngine coordinating extraction across Rayon
│   └── dedup_engine.rs          # MediaDeduplicationEngine orchestrating 4-tier pipeline and ranking
└── main.rs                      # CLI entry point (`scan`, `extract`, `extract-stream`, `dedup`, `dedup-stream`)
```
