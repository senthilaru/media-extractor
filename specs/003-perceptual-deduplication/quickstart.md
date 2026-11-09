# Quickstart & Validation Guide: Tiered Media Deduplication

**Feature**: `003-perceptual-deduplication`  
**Date**: 2026-11-07  
**Status**: Ready for Planning  

---

## 1. Prerequisites

- **Rust Toolchain**: Rust 1.82+ with `cargo`.
- **Target OS**: macOS (Apple Silicon M1/M2/M3 or Intel) or Linux (x86_64 / arm64).

---

## 2. Setup & Build

Compile in release mode to verify compiler flags and optimization:

```bash
cargo check
cargo build --release
```

---

## 3. End-to-End Validation Scenarios

### Scenario 1: Exact Bitwise Deduplication
Find exact byte-for-byte duplicate photos and videos across nested subdirectories:

```bash
cargo run --release -- dedup /Volumes/SP-1/DCIM --mode exact --format json
```

**Expected Outcome**:
- Groups files with identical BLAKE3 hashes into clusters.
- Emits reclaimable storage metrics.
- Prunes all size-mismatched files at Tier 1 without reading content.

### Scenario 2: Perceptual Near-Duplicate Clustering
Identify resized thumbnails, recompressed copies, and rotated variants:

```bash
cargo run --release -- dedup /Volumes/SP-1/DCIM \
  --mode perceptual \
  --similarity-threshold 10 \
  --format json
```

**Expected Outcome**:
- Clusters near-duplicates where Hamming distance $\le 10$ bits (similarity $\ge 84.4\%$).
- Automatically identifies 90°, 180°, and 270° rotated variants.
- Flags low-entropy blank images to prevent false-positive grouping.
- Selects the highest quality canonical file (RAW / highest resolution).

### Scenario 3: End-to-End Unix Streaming Pipeline
Chain the directory scanner, metadata extractor, and deduplication engine into a memory-bounded streaming pipeline:

```bash
cargo run --release -- scan /Volumes/SP-1 --format json -q | \
  cargo run --release -- extract-stream --format json | \
  cargo run --release -- dedup-stream --similarity-threshold 10 > duplicates.ndjson
```

**Expected Outcome**:
- Emits NDJSON records where each line is an independently parseable `DuplicateCluster`.
- Pipeline memory consumption remains strictly bounded (< 100 MB).

---

## 4. Running Automated Tests & Benchmarks

```bash
# Run unit and integration tests
cargo test

# Run deduplication performance benchmarks
cargo bench --bench dedup_bench
```
