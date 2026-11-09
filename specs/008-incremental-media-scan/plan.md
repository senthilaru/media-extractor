# Implementation Plan: Incremental Media Scan & Database Change Caching

**Branch**: `008-incremental-media-scan` | **Date**: 2026-11-09 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/008-incremental-media-scan/spec.md`

## Summary

This feature eliminates redundant extraction computation and disk I/O when re-scanning a previously indexed directory. It introduces:
1. **Database Batch Pre-Filtering**: Before extracting media metadata, batches of discovered items are checked against the unique index on `file_context.file_path` in MongoDB.
2. **Change Detection**: Compares filesystem size and `modified_at` (mtime) against database values. Unmodified files skip container parsing, EXIF extraction, and hashing entirely.
3. **Selective Extraction**: Automatically isolates new or modified files for extraction and persistence.
4. **Force Override**: Provides `--force` / `--reindex` flag to bypass incremental skipping when desired.
5. **Observability**: Live terminal progress and final execution summary display skipped/cached counts and effective throughput.

---

## Technical Context

**Language/Version**: Rust 2021 (Rust 1.80+)  
**Primary Dependencies**:
- `mongodb = "3.2"` (bulk `$in` index queries and updates)
- `tokio = { version = "1.41", features = ["full"] }` (asynchronous pipeline)
- `indicatif = "0.17"` (terminal progress bar displaying skipped/cached counts)
- `clap = { version = "4.5", features = ["derive", "env"] }` (CLI `--force` / `--reindex` parsing)

**Storage**: MongoDB 4.4+ (unique B-Tree index on `file_context.file_path`)  
**Testing**: `cargo test` (unit and integration tests)  
**Target Platform**: macOS (Apple Silicon M1 optimized), Linux  
**Project Type**: CLI tool & high-throughput asynchronous media pipeline  
**Performance Goals**:
- Incremental re-scan on 10,000+ unchanged files completes in $< 5$ seconds (>95% time reduction).
- Pre-filtering throughput $> 2,000$ items/sec over local/fast network database connection.
**Constraints**:
- Strictly 100% safe Rust.
- Zero disk read operations for unmodified cached media files.

---

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Check | Status |
|---|---|---|
| **I. High-Throughput I/O & Hardware-Conscious Concurrency** | Bypasses redundant file I/O and compute decoding for already indexed assets; keeps memory bounded. | **PASS** |
| **II. Domain-Driven Design & SOLID Boundaries** | Pre-filtering defined as a port method on `MetadataStorageSink`; domain models remain pure. | **PASS** |
| **III. Comprehensive Measurability & Observability** | Progress indicators and summary reports explicitly quantify skipped/cached files, new files, and time saved. | **PASS** |
| **IV. Idiomatic Rust & Zero-Cost Reliability** | Strongly typed error handling and zero panics; sub-second timestamp tolerance for filesystem edge cases. | **PASS** |
| **V. Fault Isolation & Resilient Batch Ingestion** | Pre-filtering failure fails fast with explicit diagnostic message; individual file errors do not abort batch. | **PASS** |
| **VI. Zero Hardcoded Credentials & Secret Sanitization** | Preserves all credential masking in database connection and telemetry. | **PASS** |

---

## Project Structure

### Documentation (this feature)

```text
specs/008-incremental-media-scan/
├── plan.md              # This plan
├── research.md          # Technical research & query decisions
├── data-model.md        # Entities: FilterEvaluationResult, FileContext updates
├── quickstart.md        # Validation scenarios
└── contracts/
    └── cli-incremental-contract.md  # CLI flags & summary contracts
```

### Source Code Changes

```text
src/
├── domain/
│   ├── metadata_models.rs               # Add modified_at to FileContext
│   ├── ports.rs                         # Add filter_unmodified_items to MetadataStorageSink
│   └── progress_models.rs               # Add items_skipped to ScanProgressSnapshot and ExecutionSummary
├── application/
│   ├── progress_tracker.rs              # Add record_skipped to ProgressTracker
│   └── pipeline.rs                      # Integrate pre-filtering before worker dispatch
├── infrastructure/
│   ├── mongodb_sink.rs                  # Implement filter_unmodified_items using $in query
│   └── terminal_progress.rs             # Render skipped counts on progress bar and summary box
└── main.rs                              # Add --force flag and pass to pipeline profile
```

---

## Complexity Tracking

No constitutional violations or design exceptions required.
