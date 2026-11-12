# Implementation Plan: Automated Master Ingestion and Source Deletion Marking

**Branch**: `010-ingest-to-master` | **Date**: 2026-11-09 | **Spec**: [specs/010-ingest-to-master/spec.md](file:///Users/spsenthil/Documents/repo/media/metadata-extractor/specs/010-ingest-to-master/spec.md)

**Input**: Feature specification from `/specs/010-ingest-to-master/spec.md`

---

## Summary

When files across external hard drives are evaluated and marked as `NeedsIngestToMaster` (because no copy exists yet in any canonical master directory), this feature allows an operator to connect both the primary master drive and the secondary drive simultaneously and execute `metadata-extractor ingest --source-drive <DRIVE_ID>`. The system extracts capture dates and categories, computes the destination path matching the canonical master structure (`<MasterRoot>/<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}`), safely streams files across drives, verifies bitwise size and BLAKE3 integrity, registers the new canonical master in MongoDB, and marks the source file as `MarkForRemoval` (or immediately unlinks it if `--delete-source` is supplied).

---

## Technical Context

**Language/Version**: Rust 2021 edition (LLVM 19, Apple Silicon M1 optimized)

**Primary Dependencies**: `tokio` (async runtime and streaming I/O), `blake3` (cryptographic hashing), `rayon` (multithreaded compute), `serde` / `serde_json` / `serde_yaml` (serialization), `clap` (CLI), `mongodb` (storage sink), `chrono` (temporal formatting), `indicatif` (terminal progress telemetry).

**Storage**: MongoDB 3.x+ (`media_assets` collection) storing `MediaMetadataDocument` with embedded `DeduplicationMetadata`.

**Testing**: `cargo test` (unit tests in `tests/unit/`, integration tests in `tests/`, mock storage sinks, tempdir filesystem tests).

**Target Platform**: macOS (specifically Apple Silicon external drive management) and Linux.

**Project Type**: CLI executable and modular library (`metadata-extractor`).

**Performance Goals**: Maximum bus throughput (>250MB/s on USB 3.0, >1GB/s on Thunderbolt) using 1MB bounded streaming buffers; sub-millisecond path resolution and database updates.

**Constraints**: Memory footprint < 100MB RAM regardless of transferred file sizes; zero data loss (no source file marked for removal or unlinked without 100% bitwise verification); non-destructive naming collision handling.

**Scale/Scope**: Collections up to 100,000+ media files per drive; multi-terabyte volume transfers.

---

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Requirement | Plan Status |
|---|---|---|
| **I. High-Throughput I/O & Concurrency** | Non-blocking async I/O (`tokio`) for disk traversal & copying; bounded streaming buffers | **PASS**: Uses chunked 1MB streaming buffer, zero unbounded full-file memory allocations. |
| **II. Domain-Driven Design & SOLID** | Domain entities separate from infrastructure adapters; port traits for storage & engine | **PASS**: Models in `src/domain/dedup_models.rs`, service in `src/application/ingest_service.rs`, ports extended cleanly. |
| **III. Measurability & Observability** | Progress indicators and elapsed timing on all I/O operations | **PASS**: Telemetry progress emitted during streaming file copies; elapsed time and transfer rate reported in summary. |
| **IV. Idiomatic Rust & Reliability** | Zero `unsafe`, strongly-typed errors with `thiserror`, zero unhandled panics | **PASS**: Strongly typed error variants in `DeduplicationError`; strict result handling and cleanup on failure. |
| **V. Fault Isolation & Resiliency** | Failed transfers must not crash batch execution or corrupt state | **PASS**: Per-file failure isolation; failed copies leave source as `NeedsIngestToMaster` and abort destination registration. |
| **VI. Zero Hardcoded Credentials** | Masked credentials in error traces and logging | **PASS**: `DatabaseSinkConfig::sanitized_uri()` enforced; environment variable defaults. |

---

## Project Structure

### Documentation (this feature)

```text
specs/010-ingest-to-master/
├── spec.md              # Feature specification
├── plan.md              # This file
├── research.md          # Architectural decisions & research
├── data-model.md        # Domain entities, value objects, lifecycle states
├── quickstart.md        # Runnable verification scenarios
├── contracts/
│   └── cli-ingest-contract.md # CLI syntax, arguments, terminal output contracts
├── checklists/
│   └── requirements.md  # Specification quality checklist
└── tasks.md             # Tasks definition (generated in next phase by /speckit-tasks)
```

### Source Code

```text
src/
├── domain/
│   ├── dedup_models.rs       # IngestTask, MasterIngestPlan, IngestExecutionReport, IngestOptions
│   ├── ports.rs              # MetadataStorageSink extensions (query_unmastered_candidates, etc.)
│   └── errors.rs             # DeduplicationError variants (TransferFailed, IntegrityMismatch, etc.)
├── application/
│   ├── ingest_service.rs     # MasterIngestService (compiles plan, streams copy, verifies, updates DB)
│   ├── dedup_engine.rs       # Replica-aware duplicate cluster evaluation
│   ├── drive_pruner.rs       # DrivePruningService
│   └── mod.rs                # Application module exports
├── infrastructure/
│   ├── drives_config.rs      # CanonicalPathMatcher, DrivesConfigLoader
│   └── mongodb_sink.rs       # Query unmastered documents, upsert master records
├── lib.rs                    # Re-exports of domain models and services
└── main.rs                   # CLI wiring for `metadata-extractor ingest` subcommand

tests/
├── test_master_ingest.rs     # Integration tests for dry-run, copying, integrity check, DB updates
├── test_drive_pruner.rs      # Pruning integration tests
└── test_database_dedup.rs    # Dedup integration tests
```

---

## Complexity Tracking

*No constitutional violations identified. Standard hexagonal application service pattern.*

| Violation | Why Needed | Simpler Alternative Rejected Because |
|---|---|---|
| None | N/A | N/A |
