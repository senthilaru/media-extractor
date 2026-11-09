# Implementation Plan: Unified Media Metadata & Fingerprint Scanner

**Branch**: `004-unified-media-scan` | **Date**: 2026-11-07 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/004-unified-media-scan/spec.md`

## Summary

The Unified Media Metadata & Fingerprint Scanner provides a streamlined, single-command entry point (`metadata-extractor <PATH>`) with exactly 1 mandatory positional path argument. When given a directory, it recursively traverses all subdirectories, discovers photos, videos, and camera RAW assets, extracts rich metadata (EXIF camera settings, temporal facets, dimensions, video container attributes), computes streaming BLAKE3 cryptographic digests and pure-Rust 64-bit DCT perceptual hashes (pHash) with rotation invariance, and emits structured MongoDB-ready JSON/NDJSON records to standard output or an optional output file.

## Technical Context

**Language/Version**: Rust 1.82+ (2021 edition)

**Primary Dependencies**:
- `clap` (4.5): Command-line argument parsing supporting top-level positional argument with optional subcommands.
- `jwalk` (0.8): Parallel directory traversal with work-stealing and custom sort ordering.
- `rayon` (1.10): Work-stealing CPU thread pool for parallel metadata decoding and DCT hash computation.
- `tokio` (1.41): Async runtime for bounded batch channel streaming and backpressure.
- `blake3` (1.8): SIMD-accelerated cryptographic digests (streaming 64 KB buffers) and fast sample digests.
- `image` (0.25): Pure Rust image decoders (JPEG, PNG, WebP) resizing directly to $32 \times 32$ luminance rasters.
- `nom-exif` (3.8) & `kamadak-exif` (0.6): Zero-copy photo EXIF and camera RAW preview extraction.
- `serde` (1.0) & `serde_json` (1.0): High-speed JSON serialization.
- `uuid` (1.11): Unique document identifier generation.
- `thiserror` (2.0): Strongly-typed domain and infrastructure error types.
- `tracing` (0.1): Structured logging and telemetry spans.

**Storage**: Direct streaming to standard output (`stdout`) as NDJSON or writing to a file sink via `-o / --output <PATH>`. MongoDB-ready document schema.

**Testing**: `cargo test` (unit and integration tests), `criterion` (benchmarks).

**Target Platform**: macOS (Apple Silicon M1/M2/M3 unified memory, arm64) and Linux (x86_64, aarch64).

**Performance Goals**:
- $\ge 100$ media files/sec end-to-end traversal + rich EXIF + video parsing + pHash + BLAKE3 on multi-core hardware.
- Memory usage strictly bounded under 100 MB during streaming traversal of $> 100,000$ files.

**Constraints**:
- Single-command user interface requiring only 1 mandatory path argument.
- 100% safe Rust code with zero unhandled panics on corrupt or non-media files.

## Constitution Check

*GATE: Evaluated against `.specify/memory/constitution.md`*

| Principle | Requirement | Plan Status | Evaluation Notes |
|:---|:---|:---:|:---|
| **I. High-Throughput I/O & Hardware Concurrency** | Async traversal + Rayon compute pool, bounded channels, memory $< 100\text{ MB}$ | **PASS** | `ScanEngine` produces discovered batches into backpressured channels; `MetadataExtractionEngine` extracts and hashes across Rayon threads. |
| **II. Domain-Driven Design & SOLID Boundaries** | Clean domain layer, hexagonal architecture, trait ports | **PASS** | `MediaMetadataDocument` acts as domain aggregate; `ScanEngine` and `MetadataExtractionEngine` orchestrate domain ports. |
| **III. Comprehensive Measurability & Observability** | Tracing spans, metrics counters, structured telemetry | **PASS** | Real-time scan telemetry and extraction throughput are displayed on `stderr` without interfering with `stdout` JSON data. |
| **IV. Idiomatic Rust & Zero-Cost Reliability** | Idiomatic Rust, no unhandled panics, typed `thiserror` errors, zero `unsafe` | **PASS** | 100% safe Rust, typed error enums, strict `-D warnings` and `cargo fmt` enforcement. |
| **V. Fault Isolation & Resilient Batch Ingestion** | Per-file error isolation; corrupt files never crash the process | **PASS** | Failed extractions generate structured diagnostic fallback documents with error details in `extraction_status`. |

## Project Structure

### Documentation (this feature)

```text
specs/004-unified-media-scan/
├── spec.md              # Feature specification
├── plan.md              # This implementation plan
├── research.md          # Phase 0: CLI ergonomics and output sink design
├── data-model.md        # Phase 1: Unified MediaMetadataDocument entity schema
├── quickstart.md        # Phase 1: End-to-end validation scenarios
├── contracts/           # Phase 1: CLI invocation contract and JSON schema
│   └── unified-cli-contract.md
└── checklists/
    └── requirements.md  # Specification quality checklist
```

### Source Code (repository root)

```text
src/
├── lib.rs                       # Crate root and public exports
├── domain/                      # Domain Layer (Value Objects & Models)
│   ├── mod.rs
│   ├── models.rs                # DiscoveredMediaItem, DiscoveryBatch, MediaCategory
│   ├── metadata_models.rs       # MediaMetadataDocument, FileContext, TemporalMetadata
│   ├── dedup_models.rs          # MediaFingerprint, RotatedHashes
│   ├── ports.rs                 # DirectoryScanner, MediaMetadataExtractor, MediaFingerprinter
│   └── errors.rs                # ScanError, ExtractionError, FingerprintError
├── infrastructure/              # Infrastructure Layer (Adapters)
│   ├── mod.rs
│   ├── walker.rs                # Jwalk/Rayon parallel filesystem walker
│   ├── classifier.rs            # Compile-time zero-allocation phf classifier
│   ├── cycle_detector.rs        # Inode-based circular symlink guard (dev, ino)
│   ├── telemetry.rs             # Atomic session metrics and heartbeat emitter
│   ├── exif_parser.rs           # nom-exif & kamadak-exif photo/RAW extractor
│   ├── video_parser.rs          # Video container header & atom extractor
│   ├── blake3_hasher.rs         # BLAKE3 fast sample digest and streaming cryptographic hasher
│   ├── phash.rs                 # Pure-Rust Discrete Cosine Transform (DCT) 64-bit perceptual visual hasher
│   └── raster_decoder.rs        # Bounded image/RAW decoder resizing directly to 32x32 grayscale
├── application/                 # Application Layer (Orchestration)
│   ├── mod.rs
│   ├── batch_accumulator.rs     # Bounded batch chunking and channel emission
│   ├── engine.rs                # ScanEngine coordinating traversal and backpressure
│   └── metadata_engine.rs       # MetadataExtractionEngine coordinating extraction across Rayon
└── main.rs                      # Unified CLI entry point supporting `metadata-extractor <PATH>`
```
