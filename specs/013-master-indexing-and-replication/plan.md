# Implementation Plan: Master Asset Registration and Replication Lifecycle Tracking

**Branch**: `013-master-indexing-and-replication` | **Date**: 2026-11-12 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/013-master-indexing-and-replication/spec.md`

## Summary

When ingesting media from a directory containing multiple duplicate copies into the primary master volume, the ingestion pipeline must:
1. Transfer exactly one copy into the primary master archive.
2. Register an authoritative `CanonicalMaster` document in MongoDB with primary filesystem path, complete metadata, cryptographic hash, and initial `MasterReplicationState`.
3. Bulk-update all other duplicate records in the cluster to `MarkForRemoval` linked directly to the canonical master (`canonical_master_id` and `canonical_master_path`).
4. Maintain replication state across configured replica drives (`Pending`, `PartiallyReplicated`, `Replicated`) updated during replica synchronization.

---

## Technical Context

**Language/Version**: Rust 2021 (Rust 1.75+)

**Primary Dependencies**: `tokio`, `mongodb`, `serde`, `serde_json`, `uuid`, `chrono`, `blake3`, `clap`, `tracing`

**Storage**: MongoDB (`media_index_db.media_assets` collection) and local/external filesystem volumes

**Testing**: `cargo test` (unit tests and integration tests in `tests/test_master_ingest.rs`, `tests/test_replica_sync.rs`, `tests/test_drive_pruner.rs`)

**Target Platform**: macOS (Apple Silicon M1 optimized), Linux

**Project Type**: High-performance CLI application

**Performance Goals**: Sub-second bulk index updates, streaming verified I/O, zero full-payload in-memory buffering

**Constraints**: Zero Data Loss principle, Hexagonal Architecture / DDD boundaries, masked credentials in all telemetry

**Scale/Scope**: Collections with multi-terabyte assets and millions of files across heterogeneous storage drives

---

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- [x] **I. High-Throughput I/O & Concurrency**: Streaming verified I/O, no blocking operations on async threads, backpressured channels.
- [x] **II. DDD & SOLID Boundaries**: Domain models (`MasterReplicationState`, `DeduplicationMetadata`) remain pure; storage operations abstracted behind `MetadataStorageSink`.
- [x] **III. Observability**: CLI reports exact numbers of masters created, duplicates linked, and replica targets pending.
- [x] **IV. Idiomatic Rust**: Zero unsafe code, strong typing, `cargo fmt` & `cargo clippy -D warnings` compliant.
- [x] **V. Zero Data Loss**: Canonical master verified on disk before source records marked for removal or deleted.

---

## Project Structure

### Documentation (this feature)

```text
specs/013-master-indexing-and-replication/
├── plan.md              # Implementation plan
├── research.md          # Technical decisions (Phase 0)
├── data-model.md        # Domain entities & state machine (Phase 1)
├── quickstart.md        # End-to-end validation guide (Phase 1)
├── contracts/           # Interface contracts (Phase 1)
│   ├── cli-contract.md
│   └── storage-port-contract.md
└── checklists/
    └── requirements.md  # Spec quality checklist
```

### Source Code Layout

```text
src/
├── domain/
│   ├── dedup_models.rs       # MasterReplicationState, ReplicationStatus, DeduplicationMetadata
│   ├── metadata_models.rs    # MediaMetadataDocument, FileContext
│   └── ports.rs              # MetadataStorageSink (link_canonical_masters, update_master_replication_state)
├── application/
│   ├── ingest_service.rs     # MasterIngestService (master registration, cluster linking, replication init)
│   ├── replica_service.rs    # MultiReplicaSyncService (replica synchronization, status updates)
│   └── drive_pruner.rs       # DrivePruningService (verified duplicate removal)
├── infrastructure/
│   └── mongodb_sink.rs       # MongoDbStorageSink implementation for master linking & replication state
└── main.rs                   # CLI reporting for ingest, replicate, and dedup commands
```
