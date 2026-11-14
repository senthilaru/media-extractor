# Implementation Plan: External HDD I/O Transfer & Buffer Optimization

**Branch**: `016-optimize-hdd-transfer` | **Date**: 2026-11-13 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/016-optimize-hdd-transfer/spec.md`

## Summary

Optimize file transfer performance across external mechanical hard disk drives (HDDs) by:
1. Increasing default streaming chunk buffer from 1 MB to 8 MB (bounded by file size) to saturate sequential track bandwidth and reduce context-switching syscalls.
2. Integrating continuous in-flight cryptographic BLAKE3 hashing during the streaming copy loop, completely eliminating the secondary full-disk read pass on destination drives.
3. Enforcing per-spindle serialization to eliminate mechanical head seek thrashing while permitting non-interfering parallel transfers across independent physical drives.
4. Exposing configurable `--buffer-size` and `--recheck-disk-hash` options across `ingest` and `sync-replicas`.

---

## Technical Context

**Language/Version**: Rust 1.75+ (Edition 2021)

**Primary Dependencies**: `tokio` (asynchronous file I/O & runtime), `blake3` (SIMD cryptographic hashing), `indicatif` (terminal progress tracking), `clap` (CLI argument parsing)

**Storage**: Local filesystems across external USB 3.0 / USB-C / Thunderbolt storage drives (APFS, exFAT) and MongoDB 7.0 (local metadata store)

**Testing**: `cargo test`, integration tests in `tests/test_master_ingest.rs`, `tests/test_replica_sync.rs`

**Target Platform**: macOS (Apple Silicon M1/M2/M3/M4 & Intel) and Linux

**Project Type**: High-performance CLI tool & asynchronous batch media pipeline

**Performance Goals**:
- $\ge 25\%$ increase in sustained sequential write throughput on mechanical external HDDs.
- $50\%$ reduction in physical read I/O on destination media by eliminating redundant secondary disk re-reads.
- Zero head-seeking contention on individual physical drive spindles.

**Constraints**:
- Strictly bounded memory: max 32 MB buffer allocation across active transfer streams.
- Zero data loss: storage-level commit (`sync_all` / `fsync`) before reporting task completion.

**Scale/Scope**: Multi-gigabyte single media files (4K videos up to 3 GB+) across libraries of 100k+ assets.

---

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-checked after Phase 1 design.*

| Principle / Standard | Status | Validation / Rationale |
| :--- | :---: | :--- |
| **I. High-Throughput I/O & Hardware-Conscious Concurrency** | **PASS** | 8 MB bounded buffer and per-spindle serialization directly protect against mechanical drive degradation without blocking Tokio asynchronous worker threads. Memory is bounded to `min(file_size, max_buffer)`. |
| **II. Domain-Driven Design & SOLID Boundaries** | **PASS** | `TransferBufferConfig` and `InFlightStreamHasher` encapsulate transfer policies cleanly within `application::transfer_service` behind existing domain traits. |
| **III. Comprehensive Measurability & Observability** | **PASS** | Ingest and replica sync progress bars report actual flushed bytes, live transfer speeds (`MB/s`), item counts, elapsed time, and ETA. |
| **IV. Idiomatic Rust & Zero-Cost Reliability** | **PASS** | Zero `unwrap()` in production paths; errors typed via `DeduplicationError::TransferFailed`. |
| **V. Fault Isolation & Resilient Batch Ingestion** | **PASS** | Streaming copy errors cleanly rollback incomplete target files via `tokio::fs::remove_file` without terminating the batch. |
| **VI. Zero Hardcoded Credentials** | **PASS** | Zero credentials involved; CLI parameters strictly govern I/O chunk parameters and verification flags. |

---

## Project Structure

### Documentation (this feature)

```text
specs/016-optimize-hdd-transfer/
├── plan.md              # Implementation plan (this document)
├── spec.md              # Feature specification
├── research.md          # Technical research & decisions (Phase 0)
├── data-model.md        # Domain entities & value objects (Phase 1)
├── quickstart.md        # Validation & verification scenarios (Phase 1)
├── contracts/
│   ├── cli-transfer-options.md      # CLI parameter contracts
│   └── transfer-service-contract.md # Service API contract
└── checklists/
    └── requirements.md  # Specification quality checklist
```

### Source Code Impact (repository root)

```text
src/
├── application/
│   ├── transfer_service.rs      # Stream copy with 8MB buffer, in-flight BLAKE3, sync_all
│   ├── ingest_service.rs        # Ingest options integration with TransferBufferConfig
│   └── replica_service.rs       # Replica sync options integration with TransferBufferConfig
├── domain/
│   └── dedup_models.rs          # TransferBufferConfig definition & parsers
└── main.rs                      # CLI argument parsing (--buffer-size, --recheck-disk-hash)

tests/
├── test_master_ingest.rs        # Ingest verified copy tests
├── test_replica_sync.rs         # Replica sync verified copy tests
└── test_transfer_service.rs     # Buffer tuning & in-flight hash unit tests
```

---

## Implementation Roadmap

### Phase 0: Research & Architecture Decisions *(Complete)*
- Analyzed mechanical HDD physics (actuator seek latency vs contiguous platter streaming).
- Validated 8 MB buffer sweet spot for USB 3.x UASP storage enclosures.
- Designed in-flight cryptographic hash verification to eliminate secondary disk re-reads.

### Phase 1: Design & Contracts *(Complete)*
- Defined `TransferBufferConfig`, `InFlightStreamHasher`, and CLI parameter contracts.
- Authored contract specifications and validation quickstart guide.

### Phase 2: Implementation (Tasks)
1. **Core Transfer Service**:
   - Update `UnifiedFileTransferService::stream_copy_verified` to accept `TransferBufferConfig`.
   - Allocate dynamic buffer chunk size $\min(\text{expected\_size}, \text{buffer\_size\_bytes})$.
   - Compute BLAKE3 digest in-flight during chunk iteration.
   - Enforce `dst_file.sync_all().await` and immediate handle dropping.
2. **CLI & Service Integration**:
   - Add `--buffer-size` and `--recheck-disk-hash` arguments to `IngestArgs` and `SyncReplicasArgs` in `src/main.rs`.
   - Thread `TransferBufferConfig` through `MasterIngestService` and `ReplicaSyncService`.
3. **Automated Verification**:
   - Add unit tests verifying in-flight hash correctness and rollback behavior on corruption.
   - Run end-to-end integration tests on simulated and real external drives.
