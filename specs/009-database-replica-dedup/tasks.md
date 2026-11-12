# Tasks: Database-Driven Replica-Aware Media Deduplication

**Input**: Design documents from `/specs/009-database-replica-dedup/`
**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md, data-model.md, contracts/

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (`[US1]`, `[US2]`, `[US3]`, `[US4]`)
- Exact file paths included in every task

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Dependency configuration and shared project structures

- [X] T001 Add `serde_yaml = "0.9"` dependency to Cargo.toml
- [X] T002 [P] Define `DriveConfiguration`, `DriveDefinition`, and `DriveGroupSpec` domain structs in src/domain/dedup_models.rs

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core domain types, port trait definitions, and error models required by all user stories

**⚠️ CRITICAL**: Foundational tasks must complete before user story implementation

- [X] T003 [P] Define `DeduplicationStatus` enum and `DeduplicationMetadata` in src/domain/dedup_models.rs
- [X] T004 [P] Add `DeduplicationError` and configuration error variants in src/domain/errors.rs
- [X] T005 Add `query_duplicate_groups`, `update_dedup_metadata`, and `query_removal_candidates` methods to `MetadataStorageSink` trait in src/domain/ports.rs

**Checkpoint**: Core domain models and storage ports ready.

---

## Phase 3: User Story 3 - External Drive Configuration & Canonical Path Matcher (Priority: P2)

**Goal**: Load `drives.yaml` and evaluate whether any given file path resides in a drive's canonical master folder (`<MasterRoot>/<Year>/<DD-Mon-YYYY>/<CategoryFolder>`).

**Independent Test**: Unit test with valid and malformed YAML configs and path patterns verifying correct canonical vs unorganized path classification.

- [X] T006 [P] [US3] Implement YAML/TOML configuration parser in src/infrastructure/drives_config.rs
- [X] T007 [US3] Implement `CanonicalPathMatcher` in src/infrastructure/drives_config.rs matching `<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}`
- [X] T008 [P] [US3] Unit tests for config parser and canonical path matcher in tests/unit/drives_config_tests.rs

**Checkpoint**: Drive configuration and canonical path matcher functional.

---

## Phase 4: User Story 1 & 2 - Database-Driven Deduplication & Replica-Aware Hierarchy (Priority: P1) 🎯 MVP

**Goal**: Query MongoDB for duplicate content hashes, evaluate assets against drive groups (`primary` vs `replicaX`) and master path status, and mark records directly in the database.

**Independent Test**: Pre-populate database with duplicate records across drives and verify primary master is marked `CanonicalMaster`, replica master copy is `ReplicaKeep`, redundant copies are `MarkForRemoval`, and unmastered unique files are `NeedsIngestToMaster`.

- [X] T009 [US1] Implement duplicate hash aggregation pipeline in src/infrastructure/mongodb_sink.rs
- [X] T010 [US1] Implement bulk deduplication metadata updates in src/infrastructure/mongodb_sink.rs
- [X] T011 [US2] Implement replica-aware duplicate cluster hierarchy evaluation in src/application/dedup_engine.rs
- [X] T012 [US1] Wire database-driven deduplication orchestrator in src/application/dedup_engine.rs supporting `--dry-run` and live progress telemetry
- [X] T013 [P] [US1] Integration tests for database deduplication and hierarchy ranking in tests/test_database_dedup.rs

**Checkpoint**: User Stories 1 and 2 functional as an MVP; duplicates globally identified and marked in MongoDB.

---

## Phase 5: User Story 4 - Per-Drive Removal Manifest & Safe Deletion Execution (Priority: P2)

**Goal**: Provide drive-specific prune workflow (`dedup prune --drive <ID>`) allowing operators to connect one drive at a time, preview removal candidates, and safely delete files after verifying the canonical master exists.

**Independent Test**: Connect a mock drive, run prune in `--dry-run` mode to verify no deletions occur, then run with `--execute` and verify only redundant files are removed while master is verified.

- [X] T014 [US4] Implement `DrivePruningService` in src/application/drive_pruner.rs with pre-deletion master confirmation check
- [X] T015 [US4] Add `dedup prune` CLI subcommand and arguments in src/main.rs
- [X] T016 [P] [US4] Integration test for single-drive pruning and master intactness check in tests/test_drive_pruner.rs

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: CLI integration, library exports, and validation gates

- [X] T017 Update `main.rs` to support `dedup --config <PATH>` and wire MongoDB connection fallback
- [X] T018 Export new types in src/lib.rs
- [X] T019 Run quality checks with `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check`
- [X] T020 Run complete test suite with `cargo test`

---

## Dependencies & Execution Order

- **Phase 1 (Setup)** $\rightarrow$ **Phase 2 (Foundational)**: Blocks all stories.
- **Phase 3 (US3: Config & Path Matcher)** $\rightarrow$ **Phase 4 (US1 & US2: Core Engine & DB Ingestion)**.
- **Phase 4** $\rightarrow$ **Phase 5 (US4: Pruning Service)**.
- **Phase 5** $\rightarrow$ **Phase 6 (Polish & Verification)**.
