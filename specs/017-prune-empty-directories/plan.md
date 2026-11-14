# Implementation Plan: Prune Empty Directories Cleanup

**Branch**: `017-prune-empty-directories` | **Date**: 2026-11-13 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/017-prune-empty-directories/spec.md`

## Summary

Enhance the duplicate pruning pipeline (`DrivePruningService`) to automatically detect and clean up empty directory trees left behind when duplicate files are deleted during `dedup prune --execute`. Use upward ancestor traversal from deleted file paths with depth-descending ordering, handle disposable OS metadata files (`.DS_Store`, `Thumbs.db`), strictly enforce boundary ceilings at the prune root and protected master archives, and surface empty directory counts in both dry-run preview and execution telemetry.

## Technical Context

**Language/Version**: Rust 1.80+ (2021 Edition)

**Primary Dependencies**: `std::fs`, `tokio`, `mongodb`, `clap`, `tracing`, `tempfile` (tests)

**Storage**: Local and external filesystems (spindle HDDs, SSDs), MongoDB (`media_assets` collection)

**Testing**: `cargo test --test test_drive_pruner`, `cargo test`

**Target Platform**: macOS (Apple Silicon M1 optimized), Linux

**Project Type**: CLI Application

**Performance Goals**: Upward ancestor cleanup adds $< 5\%$ overhead to prune execution; no full disk walking.

**Constraints**:
- Never delete the prune target root directory or any parent of it.
- Never delete or touch protected canonical master archives (`ArchiveProtectionGuard`).
- Delete empty directories with `std::fs::remove_dir` only when empty or containing only disposable OS metadata.
- Zero `unwrap()` calls on filesystem operations (Constitution Principle IV).

**Scale/Scope**: Cleans up to thousands of nested empty folders in seconds without head thrashing.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **Principle I (High-Throughput I/O & Hardware-Conscious Concurrency)**: PASS. Upward ancestor traversal from deleted files visits only affected subtrees, avoiding full disk rescans and preventing HDD head thrashing.
- **Principle II (Domain-Driven Design & SOLID Boundaries)**: PASS. Directory cleanup logic is isolated within the pruning application service and protected by domain invariants and `ArchiveProtectionGuard`.
- **Principle III (Comprehensive Measurability & Observability)**: PASS. Adds `empty_directories_removed` and `empty_directories_eligible` to `PruneExecutionReport` and terminal output.
- **Principle IV (Idiomatic Rust & Zero-Cost Reliability)**: PASS. Uses strongly typed results, proper `Result` propagation, safe `remove_dir`, and zero `unwrap()` in production paths.
- **Principle VI (Zero Hardcoded Credentials)**: PASS. No credentials involved.

## Project Structure

### Documentation (this feature)

```text
specs/017-prune-empty-directories/
├── plan.md              # This file (/speckit-plan output)
├── research.md          # Technical decisions and tradeoffs
├── data-model.md        # Entities, validation rules, state transitions
├── quickstart.md        # Verification scenarios
├── contracts/           # CLI output contracts
│   └── cli-prune-contract.md
└── tasks.md             # Implementation tasks (/speckit-tasks output)
```

### Source Code Layout

```text
src/
├── application/
│   └── drive_pruner.rs       # Upward empty directory cleaner & prune execution integration
├── infrastructure/
│   └── archive_guard.rs      # Archive protection boundary enforcement
└── main.rs                   # CLI presentation & prune summary formatting

tests/
└── test_drive_pruner.rs      # Unit and integration tests for empty directory cleanup
```
