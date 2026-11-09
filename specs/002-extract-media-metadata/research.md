# Research & Technology Decisions: Media Metadata Extraction Engine

**Feature**: `002-extract-media-metadata`  
**Date**: 2026-11-07  
**Status**: Completed  

---

## 1. Unified Metadata Extraction Engine

### Decision
Use `nom-exif` (v3.8+) with `serde` features as the primary pure-Rust metadata parsing engine, complemented by `kamadak-exif` (v0.6+) for specialized TIFF/EXIF structures.

### Rationale
- **Pure Safe Rust**: Zero external C/C++ libraries (such as `libexif`, `libraw`, or `ffmpeg`) and no process-spawning overhead (`exiftool`). Conforms strictly to Constitution Principle IV (Idiomatic Rust & Zero-Cost Reliability) with 100% memory-safe code.
- **Unified Photo & Video Support**: `nom-exif` natively parses:
  - Compressed photos: JPEG, HEIC/HEIF (iOS), WebP, PNG, TIFF.
  - Video containers: MP4, MOV (QuickTime), 3GP, MKV, WebM.
  - Camera RAW formats: TIFF-based RAW (Canon `CR2`, Nikon `NEF`, Sony `ARW`, Adobe `DNG`, Olympus `ORF`, Pentax `PEF`) and QuickTime-based RAW (Canon `CR3`).
- **Zero-Copy Header Inspection**: Operates by reading only the header/box atoms of the file, parsing EXIF/TIFF/QuickTime tags without decompressing multi-megabyte image pixel rasters or video frame streams.

### Alternatives Considered
- `exiftool` via CLI sub-process: Highly capable, but spawning a process per file on a multi-million file archive introduces severe process-fork latency, reducing throughput from 300+ fps to < 10 fps.
- `libexif` / `libraw` / `ffmpeg` via FFI: Introduces C dependencies, compilation friction across macOS M1 arm64 and Linux, and potential memory-safety vulnerabilities.
- `rexif`: Limited to standard JPEG files; does not support HEIC, RAW, or video containers.

---

## 2. Temporal Normalization & Calendar Faceting

### Decision
Use `chrono` (v0.4+) with `serde` to parse varied date representations into standard UTC timestamps and compute discrete calendar facets (`year`, `month`, `day`, `hour`, `minute`, `second`).

### Rationale
- **Multi-Format Date Normalization**: Mobile devices and cameras emit timestamps in different formats:
  - EXIF standard: `YYYY:MM:DD HH:MM:SS` (without timezone).
  - QuickTime / MP4: Seconds since UTC midnight, Jan 1, 1904.
  - XMP / ISO 8601: `YYYY-MM-DDTHH:MM:SSZ` or with timezone offset `+HH:MM`.
  - Fallback: Filesystem modification time (`SystemTime`).
- **Database Index Optimization**: Computing integer facets (`year: 2026`, `month: 10`, `day: 14`) directly in the document enables MongoDB to execute high-speed range queries, aggregations, and timeline grouping without expensive runtime string parsing.

### Alternatives Considered
- Storing unparsed raw date strings: Pushes formatting complexity to database queries and breaks timeline indexing.
- Manual regex string slicing: Fragile when encountering non-standard camera date strings or timezone offsets.

---

## 3. Concurrency & Pipeline Integration on Apple Silicon (M1)

### Decision
Bridge the directory scanner's batch stream (`tokio::sync::mpsc::Receiver<DiscoveryBatch>`) to a Rayon work-stealing thread pool where each file's metadata is extracted in parallel, bounded by an async semaphore or channel backpressure.

### Rationale
- **CPU-Bound Offloading**: While filesystem walking is I/O-bound, metadata parsing (byte seeking, tag decoding, GPS conversion) is CPU-bound. Offloading batch extraction to Rayon worker threads prevents blocking the Tokio async executor, maintaining smooth pipeline flow.
- **M1 Hardware Saturation**: Distributes extraction across the 8 cores of Apple Silicon M1, easily surpassing the target throughput of $\ge 250$ files/sec per core.

### Alternatives Considered
- Parsing sequentially inside the async stream loop: Bottlenecks throughput to single-core capacity.
- Spawning unbounded Tokio tasks per file: Leads to executor queue saturation and high context-switching overhead.

---

## 4. Large Video File Streaming Atom Reader

### Decision
Inspect MP4/MOV container headers by reading the first 128 KB for the `ftyp` and `moov` atoms. If `moov` is located at the end of the file (common in smartphone video recordings), seek directly to the file tail using bounded random access rather than buffering the entire video.

### Rationale
- Multi-gigabyte 4K/8K video files (e.g. 10 GB MOV from an iPhone) would cause immediate out-of-memory crashes if read sequentially. Bounded seeking reads $< 256 \text{ KB}$ of metadata, completing in $< 10 \text{ ms}$ per file.

---

## 5. Fault Isolation & Error Quarantine

### Decision
Wrap extraction in typed domain `Result`s. If a file has corrupted EXIF headers or unknown sub-tags:
1. Preserve all valid file context (`scanned_root`, `file_path`, `file_size_bytes`).
2. Fall back to filesystem modification time for calendar facets.
3. Mark `extraction_status.status = "Partial"` or `"Warning"` with the diagnostic reason.
4. Emit the valid JSON record and continue batch execution without panic.

### Rationale
Adheres to Constitution Principle V (Fault Isolation). Unassisted crawls over millions of files must never crash on corrupted camera payloads.
