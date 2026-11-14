# Implementation Plan: Database Content & Health Audit Report

**Branch**: `020-database-content-report` | **Date**: 2026-11-14 | **Spec**: [spec.md](file:///Users/spsenthil/Documents/repo/media/media-extractor/specs/020-database-content-report/spec.md)

**Input**: Feature specification from `/specs/020-database-content-report/spec.md`

## Summary

Add a dedicated CLI reporting command (`cargo run -- report` / `status` / `audit`) that aggregates and displays media catalog inventory and health status from MongoDB. The report details total file count, breakdown by file extensions, counts of copied canonical masters, pending replica syncs, detected duplicates, assets marked for deletion, assets missing EXIF timestamps, and extraction error assets. It also provides an inspection subcommand (`report inspect <category>`) displaying the top 3 file paths for manual verification and renders intelligent next-action CLI command suggestions based on current catalog health.

## Technical Context

**Language/Version**: Rust 2021 (MSRV 1.80+)

**Primary Dependencies**: `tokio` 1.41 (async runtime), `mongodb` 3.2 (database driver & aggregation engine), `clap` 4.5 (CLI parser), `serde` / `serde_json` 1.0 (serialization & JSON export), `chrono` 0.4 (temporal handling), `uuid` 1.11, `indicatif` 0.17 (progress/spinner), `tracing` 0.1 (structured telemetry)

**Storage**: MongoDB 3.2+ collection `media_metadata`

**Testing**: `cargo test` (unit tests for recommendation heuristics, formatting, and domain models; integration tests for MongoDB aggregation and CLI arguments)

**Target Platform**: macOS (Apple Silicon M1 optimized) / Linux

**Project Type**: CLI application (`metadata-extractor`)

**Performance Goals**: Aggregate catalog audit summary in < 3 seconds for 1,000,000 documents; sample inspection queries in < 1 second

**Constraints**: Read-only database operations; non-blocking asynchronous queries; bounded memory usage via server-side aggregation pipelines; credential masking in all diagnostics

**Scale/Scope**: Catalogs containing up to millions of media assets across primary and replica storage drives

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- [x] **I. High-Throughput I/O & Hardware-Conscious Concurrency**: Audit queries execute non-blocking server-side MongoDB aggregations via `tokio`. No raw collection dumps or unbounded memory buffers on the client.
- [x] **II. Domain-Driven Design & SOLID Boundaries**: Report domain entities (`CatalogAuditReport`, `CommandRecommendation`, `InspectionSample`) reside purely in `domain/audit_models.rs`. Data access is abstracted behind the `StorageSink` port in `domain/ports.rs`.
- [x] **III. Comprehensive Measurability & Observability**: Structured tracing spans measure query execution times and emit completion summaries (`timetaken`, document count). Database connection URIs automatically mask credentials.
- [x] **IV. Idiomatic Rust & Zero-Cost Reliability**: Zero `unsafe` code. Uses strongly typed `StorageError` and domain models. Verified via `cargo fmt` and `cargo clippy`.
- [x] **Performance & Operational Standards**: Aggregation pipeline utilizes indexed fields (`dedup.status`, `temporal.source`, `file_context.extension`, `extraction_status.status`). Sampling uses strict projections with `.limit(3)` for immediate responsiveness.

## Project Structure

### Documentation (this feature)

```text
specs/020-database-content-report/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output: MongoDB aggregation, sample inspection, CLI design
├── data-model.md        # Phase 1 output: CatalogAuditReport, InspectionSample, CommandRecommendation
├── quickstart.md        # Phase 1 output: Validation scenarios and execution guides
├── contracts/           # Phase 1 output: CLI contract and JSON export schema
│   ├── cli-contract.md
│   └── report-json-schema.json
└── checklists/
    └── requirements.md  # Spec quality checklist
```

### Source Code (repository root)

```text
src/
├── application/
│   ├── audit_service.rs       # Orchestrates audit compilation, inspection sampling, and next-action recommendation
│   └── mod.rs
├── domain/
│   ├── audit_models.rs        # Domain models: CatalogAuditReport, ExtensionCount, InspectionCategory, InspectionSample, CommandRecommendation
│   ├── ports.rs               # StorageSink port extensions for generate_audit_report & query_inspection_samples
│   └── mod.rs
├── infrastructure/
│   ├── mongodb_sink.rs        # MongoDB aggregation pipeline for catalog audit & sample projection queries
│   └── mod.rs
└── main.rs                    # Clap CLI definition for Report subcommand and terminal formatting
tests/
├── unit/
│   └── audit_recommendation_tests.rs
└── integration/
    └── report_cli_tests.rs
```

**Structure Decision**: Single project layout matching existing Rust crate conventions. Domain entities and ports remain pure; infrastructure adapters handle database interaction; application service coordinates business logic; CLI renders outputs.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

*(No violations. All principles and standards are satisfied without exception.)*
