# Implementation Plan: Direct MongoDB Storage Sink

**Branch**: `005-mongodb-storage-sink` | **Date**: 2026-11-08 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/005-mongodb-storage-sink/spec.md`

## Summary

The Direct MongoDB Storage Sink enables the unified scanner (`metadata-extractor <PATH>`) to stream extracted metadata records directly into a local or remote MongoDB database via `--mongodb-uri` and `--collection` arguments. It implements an asynchronous, decoupled storage port (`MetadataStorageSink`) with an unordered bulk upsert adapter (`MongoDbStorageSink`), providing fail-fast connection/credential validation, automatic index initialization (unique path, temporal facets, and cryptographic/perceptual hashes), idempotent re-scan synchronization, bounded memory backpressure (< 100 MB), and masked credential logging.

## Technical Context

**Language/Version**: Rust 1.82+ (2021 edition)

**Primary Dependencies**:
- `mongodb` (3.2+): Official MongoDB Rust driver configured with `tokio-runtime` for connection pooling, BSON serialization, authentication, and bulk writes.
- `tokio` (1.41): Async runtime for non-blocking database communication and bounded batch streaming.
- `clap` (4.5): CLI parser with top-level `--mongodb-uri` and `--collection` options.
- `serde` (1.0) & `serde_json` (1.0): Direct document serialization.
- `thiserror` (2.0): Strongly-typed domain storage error representations.
- `tracing` (0.1): Structured logging and telemetry with credential sanitization.

**Storage**: Local or remote MongoDB instance (4.4+) specified via connection URI (e.g. `mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db?authSource=media_index_db`).

**Testing**: `cargo test` (unit tests with mock storage sinks, integration tests with local or test MongoDB instances), `cargo clippy`, and `cargo fmt`.

**Target Platform**: macOS (Apple Silicon M1/M2/M3, arm64) and Linux (x86_64, aarch64).

**Performance Goals**:
- $\ge 500$ documents/sec ingestion throughput into local MongoDB instances.
- $< 2$ second fail-fast connection & authentication check during startup.
- Sub-millisecond indexed queries on `file_context.file_path`, `temporal.year/month`, and `fingerprint.cryptographic_hash`.

**Constraints**:
- Memory consumption strictly bounded under 100 MB during continuous ingestion of $> 50,000$ media files.
- Decoupled Hexagonal Architecture: domain layer MUST NOT depend directly on `mongodb` or `bson` crates.
- 100% safe Rust code with zero unhandled panics on database timeouts or transient drops.
- Credentials and passwords in URIs MUST always be masked in logs, telemetry, and error outputs.

## Constitution Check

*GATE: Evaluated against `.specify/memory/constitution.md`*

| Principle | Requirement | Plan Status | Evaluation Notes |
|:---|:---|:---:|:---|
| **I. High-Throughput I/O & Hardware Concurrency** | Non-blocking async I/O, backpressured channels, memory $< 100\text{ MB}$ | **PASS** | Uses official asynchronous `mongodb` client with Tokio; batches are streamed across backpressured MPSC channels without unbounded memory buffering. |
| **II. Domain-Driven Design & SOLID Boundaries** | Clean domain layer, hexagonal architecture, trait ports | **PASS** | Domain defines pure `MetadataStorageSink` port trait in `src/domain/ports.rs`; concrete MongoDB adapter lives strictly in `src/infrastructure/mongodb_sink.rs`. |
| **III. Comprehensive Measurability & Observability** | Tracing spans, metrics counters, structured telemetry | **PASS** | Live telemetry heartbeats on `stderr` report total ingested, rate (docs/sec), queue depth, and errors; logs sanitize URIs. |
| **IV. Idiomatic Rust & Zero-Cost Reliability** | Idiomatic Rust, no unhandled panics, typed `thiserror` errors, zero `unsafe` | **PASS** | Strongly-typed `StorageError` enum; deterministic error propagation; zero `unsafe`. |
| **V. Fault Isolation & Resilient Batch Ingestion** | Error isolation; DB hiccups do not crash the scan process | **PASS** | Transient writes retry with exponential backoff; persistent failures increment error tallies without aborting traversal. |

## Project Structure

### Documentation (this feature)

```text
specs/005-mongodb-storage-sink/
├── spec.md              # Feature specification
├── plan.md              # This implementation plan
├── research.md          # Phase 0: MongoDB driver selection and bulk upsert strategy
├── data-model.md        # Phase 1: DatabaseSinkConfig, BatchWriteStats, and StorageError entities
├── quickstart.md        # Phase 1: End-to-end validation scenarios
├── contracts/           # Phase 1: CLI invocation contract and telemetry specification
│   └── mongodb-cli-contract.md
└── checklists/
    └── requirements.md  # Specification quality checklist
```

### Source Code (repository root)

```text
src/
├── lib.rs                       # Crate root and public exports
├── domain/                      # Domain Layer (Pure Domain Models & Ports)
│   ├── mod.rs
│   ├── models.rs                # DiscoveredMediaItem, DiscoveryBatch, MediaCategory
│   ├── metadata_models.rs       # MediaMetadataDocument, FileContext, TemporalMetadata
│   ├── dedup_models.rs          # MediaFingerprint, RotatedHashes
│   ├── ports.rs                 # Adds `MetadataStorageSink` trait port
│   └── errors.rs                # Adds `StorageError` strongly-typed enum
├── infrastructure/              # Infrastructure Layer (Adapters)
│   ├── mod.rs
│   ├── walker.rs                # Filesystem walker
│   ├── classifier.rs            # Media classifier
│   ├── cycle_detector.rs        # Inode circular symlink guard
│   ├── telemetry.rs             # Session metrics and telemetry
│   ├── exif_parser.rs           # Photo/RAW EXIF extractor
│   ├── video_parser.rs          # Video container parser
│   ├── blake3_hasher.rs         # BLAKE3 cryptographic and sample hasher
│   ├── phash.rs                 # 64-bit DCT perceptual visual hasher
│   ├── raster_decoder.rs        # 32x32 luminance thumbnail decoder
│   └── mongodb_sink.rs          # NEW: MongoDbStorageSink adapter implementing MetadataStorageSink
├── application/                 # Application Layer (Orchestration)
│   ├── mod.rs
│   ├── batch_accumulator.rs     # Bounded batch chunking
│   ├── engine.rs                # ScanEngine coordinating traversal
│   └── metadata_engine.rs       # MetadataExtractionEngine coordinating extraction
└── main.rs                      # Adds `--mongodb-uri` and `--collection` CLI flags and sink dispatch
```

tests/
├── integration.rs
├── integration/
│   ├── traversal_tests.rs
│   ├── backpressure_tests.rs
│   ├── resilience_tests.rs
│   ├── metadata_pipeline_tests.rs
│   ├── dedup_pipeline_tests.rs
│   ├── unified_scan_tests.rs
│   └── mongodb_sink_tests.rs    # NEW: MongoDB integration and mock sink tests
└── unit/
    └── mongodb_config_tests.rs  # NEW: URI parsing, sanitization, and config validation tests
```
