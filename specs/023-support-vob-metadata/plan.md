# Implementation Plan: Support for VOB Video Extensions and DVD Metadata Files

**Branch**: `023-support-vob-metadata` | **Date**: 2026-11-16 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/023-support-vob-metadata/spec.md`

## Summary

Add comprehensive support for DVD-Video files (`.vob`, e.g. `VTS_01_1.VOB`, `VIDEO_TS.VOB`) and their associated navigation and metadata sidecar files (`.ifo`, `.bup`). Route `.vob` to `MediaCategory::Video` and `.ifo`/`.bup` to `MediaCategory::Sidecar`. Support multi-companion sidecar detection, DVD title set stem pairing (`VTS_xx_y.VOB` matching `VTS_xx_0.IFO`/`.BUP`), master ingestion co-location in `Videos/`, replica replication, and source pruning.

## Technical Context

**Language/Version**: Rust 1.82+ (Edition 2021)
**Primary Dependencies**: `phf`, `tokio`, `blake3`, `mongodb`, `tracing`
**Storage**: MongoDB (`media_assets` collection) + Local Filesystem (POSIX / macOS)
**Testing**: `cargo test` (unit, integration, and property tests)
**Target Platform**: macOS (Apple Silicon M-series optimized), Linux
**Project Type**: CLI application and asynchronous processing library
**Performance Goals**: Zero-allocation hot-path classification via `phf`, bounded stream transfer
**Constraints**: Pure Rust implementation, zero data loss, strict canonical master protection

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **Principle I: High-Throughput I/O & Hardware-Conscious Concurrency**: PASS. Transfers use bounded chunk buffers (`TransferBufferConfig`); classification uses compile-time PHF hashing.
- **Principle II: Domain-Driven Design & SOLID Boundaries**: PASS. Clean separation between `domain/` models, `application/` transfer & ingest services, and `infrastructure/` classifiers.
- **Principle III: Comprehensive Measurability & Observability**: PASS. Operations emit tracing spans and update execution reports.
- **Principle IV: Idiomatic Rust & Zero-Cost Reliability**: PASS. Error handling via domain enums, no `unwrap()` on untrusted input.
- **Principle V: Fault Isolation & Resilient Batch Ingestion**: PASS. If a VOB has no metadata or metadata has no VOB, both are handled gracefully.

## Project Structure

### Documentation (this feature)

```text
specs/023-support-vob-metadata/
├── plan.md              # Implementation plan
├── research.md          # Technical research & decisions
├── data-model.md        # Classification & sidecar models
├── quickstart.md        # Run and validation guide
├── contracts/
│   └── vob-dvd-contract.md # Service interfaces
└── tasks.md             # Execution task breakdown
```

### Source Code

```text
src/
├── domain/
│   ├── models.rs                # MediaCategory enum & helpers
│   └── dedup_models.rs          # SidecarTransferResult
├── infrastructure/
│   ├── classifier.rs            # EXTENSION_MAP with vob, ifo, bup
│   └── video_parser.rs          # VideoMetadataExtractor codec fallback for vob
├── application/
│   ├── transfer_service.rs      # find_companion_sidecars & DVD stem matcher
│   ├── ingest_service.rs        # Ingest sidecar co-location into Videos/
│   └── drive_pruner.rs          # Multi-companion sidecar pruning
tests/
└── test_vob_dvd_metadata.rs     # Integration test suite
```
