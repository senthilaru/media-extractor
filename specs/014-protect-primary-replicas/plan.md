# Implementation Plan: Primary and Replica Archive Immutability & Deletion Protection

**Branch**: `014-protect-primary-replicas` | **Date**: 2026-11-12 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/014-protect-primary-replicas/spec.md`

## Summary

Enforce complete, unbypassable deletion protection across all Primary Master and Replica backup drives at all stages of execution. Introduce `ArchiveProtectionGuard` with canonicalized path evaluation, fail-fast validation in `dedup prune`, candidate disqualification during manifest generation, and a runtime circuit breaker before any file deletion.

---

## Technical Context

**Language/Version**: Rust 2021 (Rust 1.75+)

**Primary Dependencies**: `std::fs`, `std::path`, `thiserror`, `tracing`

**Storage**: Local filesystems, external mounted storage volumes, MongoDB

**Testing**: `cargo test` in `tests/test_drive_pruner.rs`, `tests/test_master_ingest.rs`

**Target Platform**: macOS, Linux

**Project Type**: CLI Application

**Constraints**: Zero Data Loss principle, fail-fast safety checks, canonicalized symlink resolution

---

## Constitution Check

- [x] **I. High-Throughput I/O**: Path safety checks run in sub-millisecond time without blocking async executors.
- [x] **II. DDD & SOLID**: Immutability logic encapsulated in `ArchiveProtectionGuard` and strongly typed error enum.
- [x] **III. Observability**: Clear warnings logged when deletion attempts on protected paths are rejected.
- [x] **IV. Idiomatic Rust**: Strongly typed errors, zero unsafe code.
- [x] **V. Zero Data Loss**: Absolute immutability for primary master and replica archives.

---

## Project Structure

### Documentation

```text
specs/014-protect-primary-replicas/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   └── cli-contract.md
└── checklists/
    └── requirements.md
```

### Source Code Layout

```text
src/
├── domain/
│   ├── errors.rs             # DeduplicationError::PruneForbiddenOnProtectedArchive
│   └── dedup_models.rs       # DriveRules (allow_pruning)
├── infrastructure/
│   ├── archive_guard.rs      # ArchiveProtectionGuard (canonical path immutability checks)
│   └── mod.rs
├── application/
│   ├── drive_pruner.rs       # Fail-fast validation, candidate filtering, safe deletion
│   └── ingest_service.rs     # Source deletion circuit breaker
```
