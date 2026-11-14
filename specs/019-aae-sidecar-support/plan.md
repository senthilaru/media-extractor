# Implementation Plan: Apple AAE Sidecar File Support

**Branch**: `019-aae-sidecar-support` | **Date**: 2026-11-13 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/019-aae-sidecar-support/spec.md`

## Summary

Enable transparent, end-to-end support for Apple `.AAE` edit sidecar files across all stages of media organization:
1. Detect and classify `.AAE` files as companion sidecars (`MediaCategory::Sidecar`).
2. Proactively co-locate and ingest `.AAE` sidecars directly adjacent to primary media files in the canonical master archive.
3. Synchronize `.AAE` sidecars to replica backup drives to maintain 100% data parity.
4. Prune confirmed archived `.AAE` sidecar files during source cleanup, allowing empty directories to be cleanly removed.

## Technical Context

**Language/Version**: Rust 1.75+ (2021 Edition)

**Primary Dependencies**: `tokio`, `blake3`, `phf`, `walkdir` (or native `std::fs`), `serde`

**Storage**: Local filesystems (APFS/exFAT) and MongoDB (`media_assets` collection)

**Testing**: `cargo test`, unit tests, integration tests with mock storage sinks

**Target Platform**: macOS (Apple Silicon M1 optimized), Linux

**Project Type**: CLI tool & asynchronous batch media processing pipeline

**Performance Goals**: Zero noticeable latency impact on file ingestion; streaming I/O with bounded buffers.

**Constraints**: Strict adherence to Constitution Principle IV (zero `unwrap()` calls on external inputs), Principle I (bounded memory), and Principle V (fault isolation).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **Principle I: High-Throughput I/O & Hardware-Conscious Concurrency**: PASS. Sidecar transfers use the existing bounded stream transfer buffer (`TransferBufferConfig`).
- **Principle II: Domain-Driven Design & SOLID Boundaries**: PASS. Sidecar entities defined in `domain/`, detection and transfers in `application/`, file system adapters in `infrastructure/`.
- **Principle III: Comprehensive Measurability & Observability**: PASS. Sidecar operations tracked in execution reports (`bytes_transferred`, `sidecars_deleted`).
- **Principle IV: Idiomatic Rust & Zero-Cost Reliability**: PASS. Strongly typed enums, no unhandled panics, zero unwrap on untrusted data.
- **Principle V: Fault Isolation & Resilient Batch Ingestion**: PASS. If an `.AAE` transfer fails or file is corrupt, primary media ingestion completes safely and error is quarantined.
- **Principle VI: Zero Hardcoded Credentials**: PASS. No credentials involved.

## Project Structure

### Documentation (this feature)

```text
specs/019-aae-sidecar-support/
├── spec.md              # Feature specification
├── plan.md              # Implementation plan
├── research.md          # Technical research & decisions
├── data-model.md        # Domain entities & structs
├── contracts/           # Sidecar service contracts
│   └── sidecar-service-contract.md
├── quickstart.md        # Validation scenarios
└── checklists/
    └── requirements.md  # Spec quality checklist
```

### Source Code Touched

```text
src/
├── domain/
│   ├── models.rs                # Add MediaCategory::Sidecar
│   └── dedup_models.rs          # Sidecar transfer & prune models
├── infrastructure/
│   └── classifier.rs            # Recognize "aae" in PhfMediaClassifier
├── application/
│   ├── transfer_service.rs      # Sidecar detection and co-located copy
│   ├── ingest_service.rs        # Ingest companion AAE sidecars
│   ├── replica_service.rs       # Replicate AAE sidecars to mirrors
│   ├── drive_pruner.rs          # Prune verified AAE sidecars
│   └── organize_service.rs      # Report sidecar stats in unified summary
tests/
└── test_aae_sidecars.rs         # Integration tests for AAE sidecar lifecycle
```
