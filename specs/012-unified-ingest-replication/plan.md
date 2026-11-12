# Implementation Plan: Unified Master Ingestion and Multi-Replica Synchronization

**Branch**: `012-unified-ingest-replication` | **Date**: 2026-11-10 | **Spec**: [specs/012-unified-ingest-replication/spec.md](file:///Users/spsenthil/Documents/repo/media/metadata-extractor/specs/012-unified-ingest-replication/spec.md)

**Input**: Feature specification from `/specs/012-unified-ingest-replication/spec.md`

## Summary

Expand the media deduplication and ingestion pipeline to provide universal catalog classification across all media assets (both unique and duplicate files), bitwise-verified ingestion into the primary master drive, automated multi-replica fanout synchronization to backup drives (`replica1`, `replica2`, ...), and elimination of redundant transfer/verification code through a shared streaming transfer service.

## Technical Context

**Language/Version**: Rust 1.81+ (Edition 2021)  
**Primary Dependencies**: `tokio` 1.41 (async runtime & file I/O), `rayon` 1.10 (compute pool), `blake3` 1.8 (cryptographic hashing with SIMD/mmap), `mongodb` 3.2 (async driver), `clap` 4.5 (CLI parser), `indicatif` 0.17 (terminal progress), `chrono` 0.4 (temporal handling), `uuid` 1.11, `serde` 1.0, `thiserror` 2.0  
**Storage**: MongoDB (collection: `media_assets`), external drives mounted on macOS/Linux filesystem, `drives.yaml` (with `.env` interpolation fallback)  
**Testing**: `cargo test` (unit tests, integration test suites, `MockIngestStorageSink`, `tempfile`)  
**Target Platform**: macOS (Apple Silicon M1 optimized) and Linux  
**Project Type**: CLI application & domain service library  
**Performance Goals**: Bounded memory footprint (<100MB RAM during multi-gigabyte transfers), streaming line-rate disk copies (150-500 MB/s), live throughput and progress reporting  
**Constraints**: Zero unhandled panics (`no unwrap()/expect()`), bounded 1MB chunked file streaming, clean fault isolation (failed file copy never halts batch, failed destination files immediately purged), zero hardcoded credentials  
**Scale/Scope**: Catalogs of 1,000,000+ photo and video files spanning primary and multiple secondary/tertiary replica drives  

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle / Standard | Compliance Status | Analysis & Verification |
|---|---|---|
| **I. High-Throughput I/O & Hardware Concurrency** | **PASS** | File copying uses bounded 1MB async streaming chunks (`tokio::io`) with non-blocking I/O. Hashing offloaded to compute pool (`rayon` / `blake3` mmap). No blocking file I/O on async runtime workers. |
| **II. Domain-Driven Design & SOLID Boundaries** | **PASS** | Clear separation between Domain (`dedup_models.rs`, `ports.rs`), Application (`ingest_service.rs`, `replica_service.rs`, `transfer_service.rs`, `dedup_engine.rs`), and Infrastructure (`mongodb_sink.rs`, `drives_config.rs`, `blake3_hasher.rs`). |
| **III. Measurability & Observability** | **PASS** | Live progress bars, transfer throughput (MB/s, items/s), and elapsed time summaries implemented for both ingest and replica sync via `IndicatifTerminalProgress` and structured telemetry. |
| **IV. Idiomatic Rust & Zero-Cost Reliability** | **PASS** | Strongly typed error handling via `DeduplicationError` (`thiserror`). Zero `unwrap()` or `expect()` in production code paths. |
| **V. Fault Isolation & Resilient Batch Ingestion** | **PASS** | File transfer errors are caught per task; corrupt or partial target files are immediately deleted; failed tasks do not stop execution for remaining assets. |
| **VI. Zero Hardcoded Credentials** | **PASS** | MongoDB URI and drive mount configurations strictly resolved from environment variables (`.env`) or external config files (`drives.yaml`). Masked credentials in all log and error outputs. |
| **Operational Standard: Deduplication Hierarchy** | **PASS** | Evaluates drive group priorities (`primary` > `replica1` > `replica2` > `staging`) and canonical path structure before scheduling file moves. |
| **Operational Standard: Bounded Buffers** | **PASS** | 1MB chunked streaming buffer prevents full-file buffering of massive 4K video or RAW containers into memory. |

## Project Structure

### Documentation (this feature)

```text
specs/012-unified-ingest-replication/
├── plan.md              # This file (/speckit-plan output)
├── research.md          # Phase 0 output (/speckit-plan output)
├── data-model.md        # Phase 1 output (/speckit-plan output)
├── quickstart.md        # Phase 1 output (/speckit-plan output)
├── contracts/           # Phase 1 output (/speckit-plan output)
│   ├── cli-ingest-sync.md
│   └── transfer-and-replication-engine.md
└── tasks.md             # Phase 2 output (/speckit-tasks command)
```

### Source Code Layout

```text
src/
├── domain/
│   ├── dedup_models.rs        # DeduplicationStatus, ReplicaSyncTask, ReplicaSyncPlan, ReplicaSyncReport
│   ├── metadata_models.rs     # MediaMetadataDocument, FileContext, DeduplicationMetadata
│   ├── ports.rs               # MetadataStorageSink trait extensions (query_all_content_groups, query_canonical_masters)
│   └── errors.rs              # DeduplicationError enum variants
├── application/
│   ├── transfer_service.rs    # UnifiedFileTransferService (shared verified stream copy & collision logic)
│   ├── ingest_service.rs      # MasterIngestService (source -> primary master ingestion)
│   ├── replica_service.rs     # ReplicaSyncService (primary master -> multi-replica fanout synchronization)
│   └── dedup_engine.rs        # MediaDeduplicationEngine (universal classification for unique & duplicates)
├── infrastructure/
│   ├── drives_config.rs       # CanonicalPathMatcher & DrivesConfigLoader
│   ├── mongodb_sink.rs        # MongoDbStorageSink implementation for content queries & bulk updates
│   ├── blake3_hasher.rs       # Blake3MediaHasher implementation
│   └── terminal_progress.rs   # IndicatifTerminalProgress telemetry handler
├── lib.rs                     # Library module exports
└── main.rs                    # CLI subcommands: ingest (with --replicate), sync-replicas, dedup (--all)

tests/
├── test_master_ingest.rs      # Master ingestion unit and integration tests
├── test_replica_sync.rs       # Multi-replica synchronization tests
└── test_database_dedup.rs     # Universal catalog classification tests
```

**Structure Decision**: Retains existing Rust clean hexagonal architecture, modularizing the new replica synchronization service into `src/application/replica_service.rs`, extracting shared transfer primitives into `src/application/transfer_service.rs`, extending `src/domain/ports.rs`, and enhancing CLI dispatch in `src/main.rs`.

## Complexity Tracking

> No constitutional violations or complexity exemptions required. Architecture strictly follows existing hexagonal boundaries, error handling, and performance standards.
