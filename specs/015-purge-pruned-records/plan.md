# Implementation Plan: Purge Obsolete Database Records on Duplicate Prune

**Branch**: `015-purge-pruned-records` | **Date**: 2026-11-12 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/015-purge-pruned-records/spec.md`

## Summary

When redundant duplicate files on external/staging drives are pruned with `dedup prune --execute`, permanently delete their obsolete metadata records from MongoDB instead of marking them with `status: "Removed"`. Also purge any pre-existing `Removed` records on the target drive and update execution reporting to show `records_purged`.

---

## Technical Context

**Language/Version**: Rust 2021 (Rust 1.75+)

**Primary Dependencies**: `mongodb`, `tokio`, `tracing`, `serde`, `uuid`

**Storage**: MongoDB collection `media_metadata`, local/external storage volumes

**Testing**: `cargo test` in `tests/test_drive_pruner.rs`, `tests/test_database_dedup.rs`

**Target Platform**: macOS, Linux

**Project Type**: CLI Application

**Constraints**: Zero Data Loss for canonical masters/replicas; fail-safe sequential deletion (file deleted first, then database record deleted); hard circuit-breaking against deleting master or replica documents.

---

## Constitution Check

- [x] **I. High-Throughput I/O**: Document deletion in MongoDB uses efficient filters and bulk delete operations where applicable.
- [x] **II. DDD & SOLID**: Storage deletion operations abstracted through `MetadataStorageSink` trait.
- [x] **III. Observability**: CLI output surfaces `Database Records Purged: N`.
- [x] **IV. Idiomatic Rust**: Strongly typed error handling, no panics.
- [x] **V. Zero Data Loss**: Canonical master and replica records are strictly guarded from deletion.

---

## Project Structure

### Documentation

```text
specs/015-purge-pruned-records/
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
│   └── ports.rs              # MetadataStorageSink::delete_document, delete_documents, purge_removed_records
├── infrastructure/
│   └── mongodb_sink.rs       # MongoDB delete_one / delete_many implementations
├── application/
│   └── drive_pruner.rs       # PruneExecutionReport (records_purged), DrivePruningService::prune_drive
└── main.rs                   # CLI reporting for records purged
```
