# Implementation Plan: Media Metadata Extraction Engine

**Branch**: `002-extract-media-metadata` | **Date**: 2026-11-07 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/002-extract-media-metadata/spec.md`

## Summary

The Media Metadata Extraction Engine parses EXIF, TIFF, XMP, and container metadata across compressed photos (JPEG, PNG, HEIC), camera RAW formats (Canon CR2/CR3, Nikon NEF, Sony ARW, Adobe DNG, Fujifilm RAF, Olympus ORF), and video files (MP4, MOV, MKV, WebM) from Android, iPhone, and DSLR cameras. Built in Rust and optimized for Apple Silicon (M1) multi-core hardware, it uses pure-Rust zero-copy parsers (`nom-exif` + `kamadak-exif`) to inspect headers without decompressing full image or video rasters. It normalizes capture timestamps into UTC and integer calendar facets (`year`, `month`, `day`), captures camera hardware and exposure parameters, and emits structured JSON documents formatted for direct ingestion into document databases like MongoDB.

## Technical Context

**Language/Version**: Rust 1.82+ (2021 edition)

**Primary Dependencies**:
- `nom-exif` (3.8.0): Zero-copy pure Rust EXIF, TIFF, and MP4/MOV container metadata parser with `serde`
- `kamadak-exif` (0.6.2): Specialized pure Rust TIFF/EXIF tag extraction fallback
- `chrono` (0.4): High-performance datetime normalization, UTC conversion, and calendar faceting
- `rayon` (1.10): Work-stealing thread pool for parallel CPU-bound header decoding across M1 cores
- `tokio` (1.41): Async runtime for bounded batch pipelining and channel streaming
- `serde` (1.0) & `serde_json` (1.0): High-speed JSON serialization for MongoDB-ready document output
- `thiserror` (2.0): Strongly-typed domain and adapter error definitions
- `uuid` (1.11): Unique document identifier generation

**Storage**: In-memory streaming channels and NDJSON output files; schemas modeled for direct import into MongoDB.

**Testing**: `cargo test` (unit and integration tests with synthetic and real media fixtures), `criterion` (benchmarks).

**Target Platform**: macOS (Apple Silicon M1/M2/M3 unified memory, arm64) and Linux (x86_64, aarch64).

**Project Type**: Rust library crate extension + CLI commands (`extract`, `extract-stream`).

**Performance Goals**:
- $\ge 250$ files/sec per CPU core on compressed image and video headers without full image decoding.
- $< 50\text{ ms}$ extraction latency on large video files (> 2 GB) via bounded container atom inspection.

**Constraints**:
- Memory consumption strictly bounded under 100 MB during batch extraction.
- Zero panics or unhandled termination on corrupt, truncated, or unknown metadata tags.
- 100% safe Rust code with zero C/C++ FFI dependencies.

**Scale/Scope**: Multi-million photo and video archives on local drives and external volumes (e.g. `/Volumes/SP-1`).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Requirement | Plan Status | Evaluation Notes |
|---|---|---|---|
| **I. High-Throughput I/O & Hardware-Conscious Concurrency** | Non-blocking async (`tokio`) + compute thread pool (`rayon`), M1 core optimization, bounded memory | **PASS** | Batch extraction offloaded to Rayon worker threads without blocking Tokio executor threads; bounded channel prevents memory ballooning. |
| **II. Domain-Driven Design & SOLID Boundaries** | Clean domain layer, hexagonal architecture, traits for all infrastructure components | **PASS** | Pure domain models (`MediaMetadataDocument`, `TemporalMetadata`, `CameraMetadata`) defined in `domain/`; `MediaMetadataExtractor` port implemented by infrastructure adapters in `infrastructure/`. |
| **III. Comprehensive Measurability & Observability** | Tracing spans, metrics counters, structured machine-readable telemetry | **PASS** | `tracing` spans instrument every format parser; atomic extraction counters track throughput, successes, and warnings. |
| **IV. Idiomatic Rust & Zero-Cost Reliability** | Idiomatic Rust, no unhandled panics, typed `thiserror` errors, zero `unsafe` | **PASS** | 100% safe Rust, `clippy` clean on `-D warnings`, explicit typed `ExtractionError` enums, graceful partial recovery. |
| **V. Fault Isolation & Resilient Batch Ingestion** | Per-file error boundaries, circular symlink handling, uninterrupted batch flow | **PASS** | Corrupt EXIF or unsupported codec variations produce `ExtractionResultKind::Partial` or `Failed` diagnostic records without halting batch processing. |

## Project Structure

### Documentation (this feature)

```text
specs/002-extract-media-metadata/
├── spec.md              # Feature specification
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output: nom-exif, chrono faceting, and M1 tuning
├── data-model.md        # Phase 1 output: MediaMetadataDocument and MongoDB entity schema
├── quickstart.md        # Phase 1 output: Extraction validation guide
├── contracts/           # Phase 1 output: Rust trait ports and JSON wire schema
│   ├── metadata-extractor-api.md # MediaMetadataExtractor trait interfaces
│   └── metadata-json-schema.md   # MongoDB-ready JSON document specification
└── checklists/
    └── requirements.md  # Spec quality checklist
```

### Source Code (repository root)

```text
Cargo.toml
src/
├── lib.rs
├── domain/                      # Domain Layer
│   ├── mod.rs
│   ├── errors.rs                # ScanError + ExtractionError
│   ├── models.rs                # DiscoveredMediaItem, DiscoveryBatch, etc.
│   ├── metadata_models.rs       # MediaMetadataDocument, TemporalMetadata, CameraMetadata
│   └── ports.rs                 # DirectoryScanner, MediaMetadataExtractor traits
├── infrastructure/              # Infrastructure Layer (Adapters)
│   ├── mod.rs
│   ├── walker.rs                # Jwalk parallel walker
│   ├── classifier.rs            # Phf extension classifier
│   ├── cycle_detector.rs        # Inode tracking
│   ├── telemetry.rs             # Session metrics
│   ├── exif_parser.rs           # nom-exif & kamadak-exif photo/RAW extractor adapter
│   └── video_parser.rs          # Video container atom extractor adapter
├── application/                 # Application Layer
│   ├── mod.rs
│   ├── batch_accumulator.rs     # Bounded batching
│   ├── engine.rs                # Directory scanning use case
│   └── metadata_engine.rs       # Metadata extraction use case coordinating parsers
└── main.rs                      # CLI application supporting `scan`, `extract`, `extract-stream`

tests/
├── unit/
│   ├── classifier_tests.rs
│   ├── batching_tests.rs
│   ├── exif_parser_tests.rs     # EXIF & RAW extraction unit tests
│   └── date_facets_tests.rs     # Calendar faceting (year, month, day) unit tests
├── integration/
│   ├── traversal_tests.rs
│   ├── backpressure_tests.rs
│   ├── resilience_tests.rs
│   └── metadata_pipeline_tests.rs # End-to-end scanner-to-metadata pipeline tests
└── fixtures/
    ├── media_samples/           # Test images, videos, and RAW files
    └── corrupt_samples/         # Malformed EXIF headers and truncated files
```

**Structure Decision**:
Hexagonal/Clean Architecture layout:
- `domain/metadata_models.rs`: Normalized domain entities and value objects.
- `infrastructure/exif_parser.rs` & `video_parser.rs`: Concrete adapters for `nom-exif` and `kamadak-exif`.
- `application/metadata_engine.rs`: Orchestration layer combining parsing, calendar faceting, and fallback logic.
- `main.rs`: CLI commands for single-file extraction and pipelined streaming.

## Complexity Tracking

> *No constitutional violations. Design conforms strictly to the ratified Constitution.*

| Violation | Why Needed | Simpler Alternative Rejected Because |
|---|---|---|
| *None* | *N/A* | *N/A* |
