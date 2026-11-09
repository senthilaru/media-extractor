# Tasks: Scan Progress Telemetry & High-Throughput Performance Optimization

**Input**: Design documents from `/specs/007-scan-progress-performance/`
**Prerequisites**: plan.md (required), spec.md (required), research.md, data-model.md, contracts/

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (`[US1]`, `[US2]`, `[US3]`, `[US4]`)
- Exact file paths included in every task

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Project initialization and basic dependencies

- [X] T001 Add `indicatif = "0.17"` dependency to Cargo.toml
- [X] T002 [P] Register progress domain modules and exports in src/domain/mod.rs

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core domain models, port traits, and thread-safe progress tracker

**⚠️ CRITICAL**: Foundational tasks must complete before user story implementation

- [X] T003 [P] Create progress and performance domain models (OperationalPhase, ExtractionMode, ExtractionPerformanceProfile, ScanProgressSnapshot, ExecutionSummary) in src/domain/progress_models.rs
- [X] T004 [P] Define ProgressReporter and ProgressSink port traits in src/domain/ports.rs
- [X] T005 Implement thread-safe atomic ProgressTracker with moving-average throughput and ETA calculator in src/application/progress_tracker.rs

**Checkpoint**: Core domain models and progress tracking engine ready for story integration.

---

## Phase 3: User Story 1 - Real-Time Progress, High-Level Metrics & ETA Visibility (Priority: P1) 🎯 MVP

**Goal**: Deliver real-time terminal progress display on standard error showing items, data volume, throughput rates (items/s and MB/s), elapsed duration, and dynamic ETA, culminating in a boxed execution summary.

**Independent Test**: Run extraction on a test media directory and verify standard error displays dynamic live progress and final boxed summary while stdout remains clean.

- [X] T006 [P] [US1] Implement unit tests for ProgressTracker moving-average rates, ETA calculation, and snapshot generation in tests/test_progress_tracker.rs
- [X] T007 [US1] Implement IndicatifTerminalProgress adapter supporting interactive multi-metric progress bar with spinner, rates, items, bytes, and ETA in src/infrastructure/terminal_progress.rs
- [X] T008 [US1] Implement ExecutionSummaryFormatter in src/infrastructure/terminal_progress.rs to render boxed execution summary report on stderr upon completion
- [X] T009 [US1] Integrate ProgressTracker and IndicatifTerminalProgress into directory extraction runner in src/main.rs

**Checkpoint**: User Story 1 fully functional and independently testable as an MVP.

---

## Phase 4: User Story 2 - High-Throughput Pipelined Execution & Concurrency Optimization (Priority: P1)

**Goal**: Decouple directory discovery from extraction and persistence using bounded asynchronous channels and Rayon worker pools, eliminating batch stalling.

**Independent Test**: Run scan on a multi-batch dataset and verify directory discovery continues concurrently while extraction workers process earlier batches.

- [X] T010 [US2] Create decoupled concurrent worker pipeline engine with bounded backpressure channels in src/application/pipeline.rs
- [X] T011 [US2] Update BatchMetadataExtractor implementation in src/application/metadata_engine.rs to support configurable worker concurrency and cancellation
- [X] T012 [US2] Wire concurrent pipelined extraction engine in src/main.rs replacing sequential batch-by-batch awaiting

**Checkpoint**: User Stories 1 and 2 functional; discovery and extraction run concurrently with live progress.

---

## Phase 5: User Story 3 - Configurable Extraction & Hashing Modes for Terabyte-Scale Media (Priority: P2)

**Goal**: Provide high-speed fast mode (`--fast`) and configurable hashing size threshold (`--max-hash-size`) to skip deep bitwise reading on large video files.

**Independent Test**: Invoke `./target/release/metadata-extractor <dir> --fast` and verify metadata is extracted >10x faster without computing full-file bitwise or perceptual hashes.

- [X] T013 [P] [US3] Add `--fast`, `--mode`, `--max-hash-size`, and `--workers` CLI arguments and environment variable mappings in src/main.rs
- [X] T014 [US3] Update Blake3MediaHasher in src/infrastructure/blake3_hasher.rs to support size thresholding (max_hash_size_bytes) and memory-mapped SIMD hashing (update_mmap_rayon)
- [X] T015 [US3] Update MetadataExtractionEngine in src/application/metadata_engine.rs to accept ExtractionPerformanceProfile and skip full bitwise/perceptual hashing in Fast mode

**Checkpoint**: User Story 3 functional; large video files bypass deep hashing in fast mode.

---

## Phase 6: User Story 4 - Environment-Adaptive Progress Display & Machine-Readable Output (Priority: P3)

**Goal**: Adapt progress rendering automatically between interactive TTY, non-interactive discrete periodic log lines, structured JSON lines, and quiet mode.

**Independent Test**: Redirect stderr to a file and verify discrete 5-second log lines without ANSI escapes; run with `-q` and verify silence.

- [X] T016 [P] [US4] Implement TTY auto-detection with non-TTY discrete 5-second periodic log line fallback in src/infrastructure/terminal_progress.rs
- [X] T017 [P] [US4] Add `--json-telemetry` flag and JSON progress formatting on stderr for automated supervisors in src/infrastructure/terminal_progress.rs and src/main.rs
- [X] T018 [US4] Implement graceful Ctrl+C / SIGINT signal handler in src/main.rs to cleanly stop workers and emit the partial ExecutionSummary report

**Checkpoint**: User Story 4 complete; supports CI pipes, background logs, and clean Ctrl+C exits.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Integration testing, library exports, and compiler validation

- [X] T019 [P] Export new progress and pipeline types in src/lib.rs
- [X] T020 Integration test verifying `--fast` mode, stderr progress reporting, and clean stdout JSON redirection in tests/test_progress_performance_integration.rs
- [X] T021 Validate code cleanliness with `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check`

---

## Dependencies & Execution Order

### Phase Dependencies
- **Setup (Phase 1)**: No dependencies - executes immediately.
- **Foundational (Phase 2)**: Depends on Phase 1; blocks all user stories.
- **User Story 1 (Phase 3)**: Depends on Phase 2.
- **User Story 2 (Phase 4)**: Depends on Phase 2 and US1 progress hooks.
- **User Story 3 (Phase 5)**: Depends on Phase 2, US1, and US2.
- **User Story 4 (Phase 6)**: Depends on US1 and US3.
- **Polish (Phase 7)**: Depends on all user stories being complete.

---

## Parallel Execution Opportunities

- T002, T003, T004 can run in parallel (independent files).
- T006 [P] unit tests can be developed alongside T007.
- T013 [P], T014, and T015 can be developed in parallel across CLI, Hasher, and Engine.
- T016 [P] and T017 [P] can run in parallel within Phase 6.
- T019 [P] can run in parallel during Phase 7.
