# Implementation Plan: Media Directory Walker

**Branch**: `001-media-directory-walker` | **Date**: 2026-11-07 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/001-media-directory-walker/spec.md`

## Summary

The Media Directory Walker provides a high-throughput, asynchronous filesystem traversal engine designed to discover photo and video assets across millions of files on local directory trees or mounted external storage drives (such as `/Volumes/SP-1`, SD cards, and USB media). Built in Rust and optimized for Apple Silicon (M1) multi-core hardware, it utilizes work-stealing directory walking (`jwalk`/`rayon`) bridged to non-blocking asynchronous channels (`tokio::sync::mpsc`). It enforces strict backpressure flow control to stream items in bounded batches, maintaining memory usage under 100 MB while delivering initial batches within 2 seconds. Inaccessible directories, vanished files, and circular symlinks are isolated into structured diagnostic records without interrupting the crawl session.

## Technical Context

**Language/Version**: Rust 1.82+ (2021 edition)

**Primary Dependencies**:
- `tokio` (1.x): Asynchronous runtime for task scheduling and bounded channels
- `jwalk` (0.8): Multi-threaded work-stealing parallel filesystem walker powered by `rayon`
- `rayon` (1.10): Data-parallel work-stealing thread pool tuned to Apple Silicon M1 cores
- `phf` (0.11): Compile-time perfect hash set for zero-allocation media extension classification
- `tracing` (0.1) & `metrics` (0.24): Structured instrumentation and high-frequency counters
- `thiserror` (2.0): Strongly-typed domain and adapter error definitions
- `uuid` (1.x): Unique identifier generation for scan sessions and batches

**Storage**: Local filesystems and mounted external storage volumes (APFS, HFS+, exFAT, ext4, NTFS, e.g. `/Volumes/SP-1`) and bounded in-memory async channels; no persistent database required for directory walking.

**Testing**: `cargo test` (unit and integration tests), `criterion` (micro-benchmarks for directory traversal and classification).

**Target Platform**: macOS (Apple Silicon M1/M2/M3 unified memory, arm64) and Linux (x86_64, aarch64).

**Project Type**: Rust library crate with a companion CLI interface adhering to Hexagonal Architecture.

**Performance Goals**:
- $\ge 15,000$ files/sec traversal and classification throughput on local NVMe/SSD storage.
- First batch emitted to downstream consumers within 2 seconds of scan start.

**Constraints**:
- Memory consumption strictly bounded under 100 MB regardless of archive size (tested up to 1,000,000+ files).
- Zero panics or unhandled termination on corrupt, empty, or permission-restricted files.
- Zero-cost abstractions on hot paths (zero heap allocation for extension lookups).

**Scale/Scope**: Multi-million file libraries across deep folder hierarchies (100+ levels).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Requirement | Plan Status | Evaluation Notes |
|---|---|---|---|
| **I. High-Throughput I/O & Hardware-Conscious Concurrency** | Non-blocking async (`tokio`) + compute thread pool (`rayon`), M1 core optimization, bounded memory | **PASS** | `jwalk` with Rayon utilizes M1 performance/efficiency cores without blocking Tokio workers; bounded `mpsc` channel enforces backpressure. |
| **II. Domain-Driven Design & SOLID Boundaries** | Clean domain layer, hexagonal architecture, traits for all infrastructure components | **PASS** | Pure domain models (`DiscoveredMediaItem`, `DiscoveryBatch`, `ScanSession`) defined in `domain/`; `DirectoryScanner` and `MediaClassifier` ports implemented by adapters in `infrastructure/`. |
| **III. Comprehensive Measurability & Observability** | Tracing spans, metrics counters, structured machine-readable telemetry | **PASS** | Atomic counters on hot paths; periodic telemetry heartbeats emitted via Tokio channels and `tracing` spans on all external interactions. |
| **IV. Idiomatic Rust & Zero-Cost Reliability** | Idiomatic Rust, no unhandled panics, typed `thiserror` errors, zero `unsafe` | **PASS** | 100% safe Rust, `clippy` clean on `-D warnings`, explicit typed `ScanError` enums. |
| **V. Fault Isolation & Resilient Batch Ingestion** | Per-file error boundaries, circular symlink handling, uninterrupted batch flow | **PASS** | Permission errors, missing paths, and symlink loops quarantined to `ScanErrorRecord` stream; walker continues processing remaining paths. |

## Project Structure

### Documentation (this feature)

```text
specs/001-media-directory-walker/
├── spec.md              # Feature specification
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output: Traversal engine, backpressure, and M1 tuning
├── data-model.md        # Phase 1 output: Entities, value objects, and state transitions
├── quickstart.md        # Phase 1 output: Build and end-to-end verification guide
├── contracts/           # Phase 1 output: Domain traits and CLI/streaming specs
│   ├── scanner-api.md   # Domain ports and Rust trait interfaces
│   └── cli-contract.md  # CLI command arguments and NDJSON wire protocol
└── checklists/
    └── requirements.md  # Spec quality checklist
```

### Source Code (repository root)

```text
Cargo.toml
src/
├── lib.rs                       # Crate root and module exports
├── domain/                      # Domain Layer (Enterprise Business Rules)
│   ├── mod.rs
│   ├── models.rs                # DiscoveredMediaItem, DiscoveryBatch, ScanSession, ScanErrorRecord
│   ├── ports.rs                 # DirectoryScanner, MediaClassifier traits
│   └── errors.rs                # ScanError, ScanErrorKind domain error types
├── infrastructure/              # Infrastructure Layer (Adapters)
│   ├── mod.rs
│   ├── walker.rs                # Jwalk/Rayon parallel traversal adapter
│   ├── classifier.rs            # Compile-time perfect hash (phf) media classifier
│   └── telemetry.rs             # Atomic metrics and periodic heartbeat emitter
├── application/                 # Application Layer (Use Cases & Orchestration)
│   ├── mod.rs
│   └── engine.rs                # ScanEngine coordinating walker, backpressure, and batching
└── main.rs                      # CLI application entry point

tests/
├── unit/
│   ├── classifier_tests.rs      # Extension matching and case-insensitivity tests
│   └── batching_tests.rs        # Batch chunking and size threshold tests
├── integration/
│   ├── traversal_tests.rs       # Multi-threaded filesystem walk and symlink cycle tests
│   └── backpressure_tests.rs    # Channel flow control and throttled consumer tests
└── fixtures/                    # Test directory trees (corrupt files, symlinks, deep nesting)

benches/
└── scanner_bench.rs             # Criterion micro-benchmarks for traversal and classification
```

**Structure Decision**:
Hexagonal/Clean Architecture layout:
- `domain/`: Contains pure business logic, models, error definitions, and traits without external runtime dependencies.
- `infrastructure/`: Houses concrete adapters (`jwalk`, `rayon`, `phf`, `tracing`).
- `application/`: Contains the `ScanEngine` use case orchestrating scanning and streaming.
- `main.rs`: Provides the CLI interface wrapping the library.

## Complexity Tracking

> *No constitutional violations. Design adheres strictly to the ratified Constitution.*

| Violation | Why Needed | Simpler Alternative Rejected Because |
|---|---|---|
| *None* | *N/A* | *N/A* |
