# Implementation Plan: Unified Organize Pipeline Subcommand

**Branch**: `018-unified-organize-pipeline` | **Date**: 2026-11-13 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/018-unified-organize-pipeline/spec.md`

## Summary

Implement a unified `cargo run -- organize` subcommand (aliases: `archive`, `pipeline`) that runs the full end-to-end media archival lifecycle:
1. Scan & Extract metadata from `--source` directory into MongoDB.
2. Universal deduplication & replica classification across the catalog.
3. Master ingestion of unique `NeedsIngestToMaster` files into primary master.
4. Fanout replication to active backup replica drives with BLAKE3 verification.
5. Pruning of redundant source files and recursive removal of empty ancestor directories.

## Technical Context

**Language/Version**: Rust 1.80+ (2021 Edition)

**Primary Dependencies**: `clap`, `tokio`, `mongodb`, `indicatif`, `tracing`

**Storage**: Local & External Drives, MongoDB

**Testing**: Unit & Integration tests in `tests/test_organize_pipeline.rs`

**Performance Goals**: Unified single-process execution; shared MongoDB connection; zero subprocess overhead.

## Constitution Check

- **Principle I (High-Throughput I/O)**: PASS. Uses streaming channels, bounded memory buffers, and spindle-aware copy routines.
- **Principle II (Domain-Driven Design)**: PASS. Pipeline coordinates existing application services via strict traits and clean abstractions.
- **Principle III (Comprehensive Observability)**: PASS. Live progress indicator per stage and comprehensive final summary table.
- **Principle IV (Zero-Cost Reliability & No Panics)**: PASS. Fail-fast error propagation guarantees no deletion occurs if earlier stages fail.
- **Principle VI (Zero Hardcoded Credentials)**: PASS. Database credentials externalized via environment variables.

## Project Structure

### Documentation

```text
specs/018-unified-organize-pipeline/
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/cli-organize-contract.md
└── tasks.md
```

### Source Code

```text
src/
├── application/
│   ├── mod.rs
│   └── organize_service.rs     # New: End-to-end organize pipeline service
├── main.rs                     # New: OrganizeArgs CLI subcommand & runner
tests/
└── test_organize_pipeline.rs   # Integration tests for end-to-end organize workflow
```
