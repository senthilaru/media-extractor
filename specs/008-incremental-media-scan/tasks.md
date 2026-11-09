# Tasks: Incremental Media Scan & Database Change Caching

**Input**: Design documents from `/specs/008-incremental-media-scan/`
**Prerequisites**: plan.md (required), spec.md (required), research.md, data-model.md, contracts/

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (`[US1]`, `[US2]`, `[US3]`, `[US4]`)
- Exact file paths included in every task

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Update file context models and extractor to capture modification timestamps

- [X] T001 [P] Enhance FileContext in src/domain/metadata_models.rs with `modified_at: Option<DateTime<Utc>>`
- [X] T002 [P] Update MetadataExtractionEngine in src/application/metadata_engine.rs to populate `modified_at` from DiscoveredMediaItem filesystem metadata

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core pre-filter entities, port traits, and skipped telemetry counters

**⚠️ CRITICAL**: Foundational tasks must complete before user story implementation

- [X] T003 [P] Define FilterEvaluationResult in src/domain/metadata_models.rs and add `filter_unmodified_items` method to MetadataStorageSink trait in src/domain/ports.rs
- [X] T004 [P] Add `items_skipped` and `bytes_skipped` tracking to ScanProgressSnapshot and ExecutionSummary in src/domain/progress_models.rs
- [X] T005 Add `record_skipped` to ProgressReporter trait in src/domain/ports.rs and implement in src/application/progress_tracker.rs

**Checkpoint**: Storage port and telemetry models ready for pre-filtering implementation.

---

## Phase 3: User Story 1 & 4 - Incremental Re-Scan Skipping Unmodified Files & Batch Pre-Filtering (Priority: P1) 🎯 MVP

**Goal**: Batch pre-filter discovered items against the database index, skipping extraction for unchanged files and displaying skipped metrics in real time.

**Independent Test**: Run scan twice against the same media folder with MongoDB configured; verify second run skips 100% of files in seconds with zero extra decoding.

- [X] T006 [US1] Implement `filter_unmodified_items` in src/infrastructure/mongodb_sink.rs using batch `$in` projection on `file_context.file_path`
- [X] T007 [P] [US1] Implement unit test for filter change detection logic in tests/test_incremental_filter.rs
- [X] T008 [US1] Update ExtractionPipeline in src/application/pipeline.rs to execute `filter_unmodified_items` before worker dispatch and record skipped telemetry
- [X] T009 [US1] Update IndicatifTerminalProgress in src/infrastructure/terminal_progress.rs to render skipped counts and rates in interactive progress and summary

**Checkpoint**: User Story 1 fully functional as an MVP; repeated scans complete in seconds.

---

## Phase 4: User Story 2 - Detection and Re-Extraction of New or Modified Files (Priority: P1)

**Goal**: Automatically detect newly added files or files with altered size or mtime, routing only changed files to extraction.

**Independent Test**: Modify one file's mtime and add a new file in a previously scanned directory; verify only those 2 files are extracted while all others are skipped.

- [X] T010 [US2] Verify size and mtime mismatch detection partitions modified files into `items_to_extract` in src/infrastructure/mongodb_sink.rs
- [X] T011 [US2] Integration test verifying new and modified files are re-extracted while unchanged files are skipped in tests/test_incremental_integration.rs

**Checkpoint**: User Stories 1 and 2 functional; media changes are accurately detected and updated.

---

## Phase 5: User Story 3 - Forced Re-Index & Cache Invalidation Control (Priority: P2)

**Goal**: Provide `--force` / `--reindex` option to bypass incremental pre-filtering and re-extract all assets on demand.

**Independent Test**: Run scan with `--force` on an already-indexed directory and verify 0 files are skipped.

- [X] T012 [P] [US3] Add `--force` (alias `--reindex`) CLI flag and `force_reindex` profile option in src/domain/progress_models.rs and src/main.rs
- [X] T013 [US3] Wire `force_reindex` bypass in ExtractionPipeline in src/application/pipeline.rs
- [X] T014 [US3] Integration test verifying `--force` causes 0 items to be skipped in tests/test_incremental_integration.rs

**Checkpoint**: User Story 3 complete; operators can force re-extraction at will.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Library exports and quality gates

- [X] T015 [P] Export FilterEvaluationResult and updated types in src/lib.rs
- [X] T016 Validate code cleanliness with `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check`

---

## Dependencies & Execution Order

### Phase Dependencies
- **Setup (Phase 1)**: No dependencies.
- **Foundational (Phase 2)**: Depends on Phase 1; blocks all user stories.
- **User Story 1 (Phase 3)**: Depends on Phase 2.
- **User Story 2 (Phase 4)**: Depends on Phase 3.
- **User Story 3 (Phase 5)**: Depends on Phase 3 and Phase 4.
- **Polish (Phase 6)**: Depends on all user stories.
