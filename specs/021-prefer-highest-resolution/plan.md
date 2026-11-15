# Implementation Plan: Resolution-Aware Deduplication Quality Selection

**Branch**: `021-prefer-highest-resolution` | **Date**: 2026-11-14 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `specs/021-prefer-highest-resolution/spec.md`

## Summary

Enhance the deduplication engine (`DedupEngine`) to prioritize high-resolution (pixel dimensions `width * height`) and larger file sizes when evaluating duplicate clusters and content groups. Inferior lower-resolution images (thumbnails, previews, downscaled copies) and heavily compressed smaller files will be systematically marked `MarkForRemoval`, ensuring the highest fidelity media assets are preserved in the master archive.

## Technical Context

**Language/Version**: Rust 2021 Edition (stable, M1 Apple Silicon native)

**Primary Dependencies**: `tokio`, `rayon`, `bson`, `mongodb`, `indicatif`, `blake3`, `image`, `kamadak-exif`

**Storage**: MongoDB (`media_index_db.media_assets` collection), local file system

**Testing**: `cargo test`, unit test suites (`tests/unit.rs`), integration test suites (`tests/integration.rs`)

**Target Platform**: macOS (Apple Silicon / Darwin) & Linux

**Project Type**: CLI tool & core processing engine

**Performance Goals**: Sub-microsecond ranking evaluation per cluster, zero regression in batch classification throughput across >38,000 groups

**Constraints**: Memory-bounded streaming, zero unwrap/expect in production code, strict preservation of Primary Master protection invariants

**Scale/Scope**: Entire catalog (tens of thousands to millions of documents)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **Principle I (High-Throughput I/O & Hardware-Conscious Concurrency)**: PASS. Quality ranking evaluates in-memory metadata structs (`width`, `height`, `file_size_bytes`); no blocking I/O or new thread overhead introduced.
- **Principle II (Domain-Driven Design & SOLID Boundaries)**: PASS. `MediaQualityRank` is a domain-level value object; storage and pipeline ports remain decoupled.
- **Principle III (Comprehensive Measurability & Observability)**: PASS. Summary reports and telemetry reflect counts and reclaimed bytes for low-resolution/small pruned assets.
- **Principle IV (Idiomatic Rust & Zero-Cost Reliability)**: PASS. Deterministic `Ord`/`PartialOrd` implementation with strongly typed enums and safe fallbacks. Zero `unsafe`.
- **Principle V (Fault Isolation & Resilient Batch Ingestion)**: PASS. Missing or corrupt dimensions safely fall back to byte size ranking without failing the batch.
- **Principle VI (Zero Hardcoded Credentials)**: PASS. No credentials used or modified.

## Project Structure

### Documentation (this feature)

```text
specs/021-prefer-highest-resolution/
├── plan.md              # Implementation Plan
├── research.md          # Technical research & decisions
├── data-model.md        # Entities, structs & state transitions
├── quickstart.md        # Validation scenarios
├── contracts/
│   └── dedup-quality-ranking.md # Functional contracts
└── tasks.md             # Execution task breakdown
```

### Source Code (repository root)

```text
src/
├── domain/
│   ├── dedup_models.rs      # MediaQualityRank struct & ordering
│   └── metadata_models.rs   # ImageDimensions helper accessors
├── application/
│   ├── dedup_engine.rs      # Resolution-aware candidate scoring & content group evaluation
│   └── organize_service.rs  # Organize pipeline integration
tests/
├── unit.rs                  # Quality rank ordering unit tests
└── test_dedup_quality.rs    # Integration tests for mixed-resolution clusters
```

**Structure Decision**: Single Rust CLI project modifying existing domain models and application deduplication engine.
