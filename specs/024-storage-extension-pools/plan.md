# Implementation Plan: Multi-Volume Storage Extension Pools

**Branch**: `024-storage-extension-pools` | **Date**: 2026-11-20 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/024-storage-extension-pools/spec.md`

## Summary

Enable horizontal storage capacity expansion for Primary Master and Replica backup archives by supporting Multi-Volume Storage Pools (`primary_vol1`, `primary_vol2`, `replica_1_vol1`, `replica_1_vol2`). Ensure that identical directory structures (`<Year>/<DD-Mon-YYYY>/{Photos,Videos,PhotosRaw}`) can span across multiple member volumes while strictly enforcing a **single-copy invariant** (zero intra-pool duplication). Implement capacity-aware spillover for ingestion, multi-volume replica synchronization, and archive protection across all configured pool volumes.

## Technical Context

**Language/Version**: Rust 1.82+ (Edition 2021)
**Primary Dependencies**: `tokio`, `blake3`, `mongodb`, `serde`, `serde_yaml`, `clap`, `tracing`, `libc`
**Storage**: MongoDB (`media_assets` collection) + POSIX Filesystems (APFS / HFS+ / ext4 / exFAT)
**Testing**: `cargo test` (unit, integration, and mock filesystem capacity tests)
**Target Platform**: macOS (Apple Silicon M-series optimized), Linux
**Project Type**: CLI application & core media discovery engine
**Performance Goals**: Microsecond disk-space probes via `libc::statvfs`, zero memory overhead on pool lookups
**Constraints**: Zero data loss, strict protection of all master/replica volumes, 100% backward compatibility with single-drive configurations

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **Principle I: High-Throughput I/O & Hardware-Conscious Concurrency**: PASS. Transfers use bounded chunk buffers (`TransferBufferConfig`); disk-space probing uses direct POSIX syscalls (`statvfs`) without blocking the async runtime.
- **Principle II: Domain-Driven Design & SOLID Boundaries**: PASS. Storage pool models defined in `domain/`, spillover and ingestion orchestration in `application/`, filesystem capacity probing behind `DiskSpaceProbe` trait in `infrastructure/`.
- **Principle III: Comprehensive Measurability & Observability**: PASS. Master ingestion and replica synchronization telemetry display target volume identifiers and free-space stats.
- **Principle IV: Idiomatic Rust & Zero-Cost Reliability**: PASS. Error handling via strongly typed domain enums (`StoragePoolExhausted`), no panics on full disks.
- **Principle V: Fault Isolation & Resilient Batch Ingestion**: PASS. If one volume in a pool reaches capacity, subsequent files spill over to the next volume cleanly without failing the broader batch.

## Project Structure

### Documentation (this feature)

```text
specs/024-storage-extension-pools/
├── plan.md              # Implementation plan
├── research.md          # Technical research & decisions
├── data-model.md        # StoragePool & DiskSpaceProbe models
├── quickstart.md        # Run and validation scenarios
├── contracts/
│   └── storage-pool-contract.md # Service interfaces
└── tasks.md             # Execution task breakdown (generated via /speckit-tasks)
```

### Source Code

```text
src/
├── domain/
│   ├── dedup_models.rs          # StoragePool helper methods on DriveConfiguration
│   └── errors.rs                # StoragePoolExhausted error variant
├── infrastructure/
│   ├── drives_config.rs         # Pool resolution & environment variable interpolation (EXT1, EXT2)
│   ├── disk_probe.rs            # DiskSpaceProbe trait & PosixDiskSpaceProbe (statvfs)
│   └── archive_guard.rs         # Protects all member volumes across all pools
├── application/
│   ├── dedup_engine.rs          # Intra-pool duplicate resolution enforcing single-copy invariant
│   ├── ingest_service.rs        # Capacity-aware volume spillover during master ingest
│   └── replica_service.rs       # Multi-volume replica pool synchronization
tests/
└── test_storage_pools.rs        # Integration test suite for multi-volume pools
```
