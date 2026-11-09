# Quickstart & Validation Guide: Media Metadata Extraction Engine

**Feature**: `002-extract-media-metadata`  
**Date**: 2026-11-07  
**Status**: Completed  

---

## 1. Prerequisites

- **Rust Toolchain**: Rust 1.82+ with `cargo`.
- **Target OS**: macOS (Apple Silicon M1/M2/M3 or Intel) or Linux (x86_64 / arm64).

---

## 2. Setup & Build

Compile the project and verify compiler warnings:

```bash
cargo check
cargo build --release
```

---

## 3. End-to-End Validation Scenarios

### Scenario 1: Extracting Metadata from Single Photo / RAW File
Extract EXIF metadata and calendar facets from an image or camera RAW file:

```bash
cargo run --release -- extract /Volumes/SP-1/DCIM/100CANON/IMG_1042.CR3
```

**Expected Outcome**:
- Output conforms to the JSON schema in [contracts/metadata-json-schema.md](contracts/metadata-json-schema.md).
- Camera `make`, `model`, `lens_model`, ISO, aperture, and shutter speed are extracted.
- Integer facets (`year`, `month`, `day`) are populated.

### Scenario 2: Extracting Video Container Metadata
Extract duration, codecs, frame rate, and rotation from an MP4 or MOV file:

```bash
cargo run --release -- extract /Volumes/SP-1/DCIM/100CANON/VID_0012.MOV
```

**Expected Outcome**:
- Output contains `video_properties` block with `duration_ms`, `video_codec`, `frame_rate_fps`, and `rotation_degrees`.

### Scenario 3: End-to-End Pipeline (Scan + Extract Streaming)
Pipe discovered batches from the directory scanner into the metadata extractor to generate MongoDB-ready NDJSON:

```bash
cargo run --release -- scan /Volumes/SP-1 --format json -q | \
  cargo run --release -- extract-stream --format json > media_metadata.ndjson
```

**Expected Outcome**:
- `media_metadata.ndjson` contains one valid JSON object per media file.
- Memory usage of the combined pipeline remains bounded (< 100 MB).

---

## 4. Running Automated Tests

```bash
cargo test
```
