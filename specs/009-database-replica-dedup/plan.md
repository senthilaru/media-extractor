# Implementation Plan: Database-Driven Replica-Aware Media Deduplication

**Branch**: `009-database-replica-dedup` | **Date**: 2026-11-09 | **Spec**: [specs/009-database-replica-dedup/spec.md](spec.md)

**Input**: Feature specification from `/specs/009-database-replica-dedup/spec.md`

## Summary

Enhance `dedup` and `dedup-stream` to execute directly over the unified media metadata maintained in MongoDB. The engine loads an external drive configuration (`drives.yaml`) that defines `primary` and `replica` drive groups and their canonical master directory paths (`.../master/<Year>/<DD-Mon-YYYY>/<CategoryFolder>`). Using indexed aggregation queries, it forms global duplicate clusters across all 10+ drives without requiring all physical drives to be connected at once. It automatically designates canonical masters, preserves authorized replica mirror copies (`ReplicaKeep`), marks redundant scattered copies for deletion (`MarkForRemoval`), and protects unique unmastered files (`NeedsIngestToMaster`). Finally, it provides an incremental drive-by-drive pruning command (`dedup prune --drive <DRIVE>`) allowing operators to safely connect one drive at a time and reclaim storage with pre-deletion master verification.

## Technical Context

**Language/Version**: Rust 1.82+ (2021 edition)

**Primary Dependencies**:
- `mongodb = "3.2"`: High-throughput aggregation pipelines, batch cursors, and bulk update writes.
- `tokio = { version = "1.41", features = ["full"] }`: Asynchronous runtime.
- `serde = { version = "1.0", features = ["derive"] }` & `serde_yaml = "0.9"`: Configuration parsing for `drives.yaml`.
- `blake3 = "1.8"`: Content fingerprint matching.
- `chrono = { version = "0.4", features = ["serde"] }`: Calendar facets and timestamp validation.
- `clap = { version = "4.5", features = ["derive", "env"] }`: CLI flags and subcommand extensions.
- `indicatif = "0.17"`: Real-time progress bars for database evaluation and drive pruning.

**Storage**: MongoDB collection `media_assets` with indexes on `fingerprint.cryptographic_hash`, `file_context.file_path`, `dedup.status`, and `dedup.cluster_id`.

**Testing**: `cargo test` (unit tests in `tests/unit/`, integration in `tests/integration/`, end-to-end tests in `tests/`).

**Target Platform**: macOS (Apple Silicon M1/M2/M3/M4) and Linux.

**Project Type**: CLI application and library.

**Performance Goals**:
- Global duplicate clustering of 50,000+ MongoDB records in $< 10$ seconds.
- Single-drive cleanup throughput $> 1,000$ files/minute.
- Memory usage strictly bounded $< 100$ MB during aggregation and batch updates.

**Constraints**:
- Absolute safety: Never mark a unique file for removal if no verified canonical master exists.
- Simulation by default: All pruning commands require explicit `--execute` flag; default is non-destructive dry-run preview.
- Non-blocking async I/O; no credential leaks.

**Scale/Scope**: 10+ external drives, 50,000–500,000 media files, 1–10 TB total media volume.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Evaluation | Status |
|---|---|---|
| **I. High-Throughput I/O & Hardware Concurrency** | Database queries use batch `$in` and server-side aggregation `$group` scans; updates run via bulk un-ordered models with bounded concurrency. | **PASS** |
| **II. DDD & SOLID Boundaries** | Pure domain models (`DriveConfiguration`, `CanonicalPathMatcher`, `DeduplicationStatus`) decoupled from MongoDB storage and CLI layer. | **PASS** |
| **III. Measurability & Observability** | Terminal telemetry outputs live progress, per-drive reclaimable space, and detailed summary tables upon completion. | **PASS** |
| **IV. Idiomatic Rust & Reliability** | Strongly-typed domain enums, zero unwraps on untrusted paths or configs, compiler and clippy warnings enforced. | **PASS** |
| **V. Fault Isolation & Resilient Batch Ingestion** | Double-confirmation check verifies canonical master intactness before permitting local deletion; fails safe on missing disks. | **PASS** |
| **VI. Zero Hardcoded Credentials** | MongoDB URI and password sourced from environment variables (`MONGODB_URI`, `MONGODB_PASSWORD`) or profiles. | **PASS** |

## Project Structure

### Documentation (this feature)

```text
specs/009-database-replica-dedup/
├── spec.md              # Feature specification & requirements
├── plan.md              # This file (/speckit-plan output)
├── research.md          # Technical research & decisions
├── data-model.md        # Entities, enums, and MongoDB schema
├── contracts/
│   ├── drives-config-schema.md # YAML schema for drives.yaml
│   └── cli-dedup-contract.md   # CLI arguments & output format
├── quickstart.md        # Verification scenarios
└── checklists/
    └── requirements.md  # Spec quality checklist
```

### Source Code Architecture

```text
src/
├── domain/
│   ├── dedup_models.rs      # Enhanced with DriveConfiguration, DeduplicationStatus, DeduplicationMetadata
│   ├── ports.rs             # DeduplicationStoragePort (query_duplicates, update_dedup_metadata)
│   └── errors.rs            # DeduplicationError & ConfigError variants
├── infrastructure/
│   ├── drives_config.rs     # YAML/TOML loader and CanonicalPathMatcher
│   ├── mongodb_sink.rs      # Extended with aggregation pipeline and batch update queries
│   └── terminal_progress.rs # Extended with deduplication & pruning progress reporting
├── application/
│   ├── dedup_engine.rs      # Enhanced with replica-aware master hierarchy ranking
│   └── drive_pruner.rs      # DrivePruningService with pre-deletion safety verification
└── main.rs                  # Extended `dedup` and `dedup prune` CLI subcommands
```

**Structure Decision**: Single project using existing Domain-Driven Design (DDD) Hexagonal architecture. All drive configuration and canonical path logic lives in pure domain modules; MongoDB aggregations live in infrastructure; orchestrations in application.

## Complexity Tracking

*No constitutional violations. Design uses standard serialization and database primitives without introducing heavy dependencies.*
