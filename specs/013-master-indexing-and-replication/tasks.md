# Tasks: Master Asset Registration and Replication Lifecycle Tracking

**Input**: Design documents from `specs/013-master-indexing-and-replication/`
**Prerequisites**: plan.md (required), spec.md (required), research.md, data-model.md, contracts/

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Verify and prepare project structure and dependencies

- [X] T001 [P] Verify and ensure dependencies in Cargo.toml are up to date for replication lifecycle tracking

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core data models and port traits required by all user stories

- [X] T002 [P] Define `ReplicationStatus` enum and `MasterReplicationState` struct in `src/domain/dedup_models.rs`
- [X] T003 [P] Add `replication: Option<MasterReplicationState>` field to `DeduplicationMetadata` in `src/domain/dedup_models.rs`
- [X] T004 Add `update_master_replication_state` and `query_masters_by_replication_status` methods to `MetadataStorageSink` trait in `src/domain/ports.rs`
- [X] T005 Implement `update_master_replication_state` and `query_masters_by_replication_status` in `src/infrastructure/mongodb_sink.rs`

**Checkpoint**: Foundation ready - domain models and storage methods compiled and ready for user stories.

---

## Phase 3: User Story 1 - Authoritative Primary Master Database Registration (Priority: P1) 🎯 MVP

**Goal**: Guarantee that every file transferred to the primary master volume is recorded in MongoDB as an authoritative `CanonicalMaster` document with full path, size, date, and hash.

**Independent Test**: Running ingest on a unique file writes the file to the master drive and registers a `CanonicalMaster` document in MongoDB with identical path, size, and BLAKE3 hash.

### Tests for User Story 1
- [X] T006 [P] [US1] Add unit test for canonical master database registration upon ingestion in `tests/test_master_ingest.rs`

### Implementation for User Story 1
- [X] T007 [US1] Ensure `MasterIngestService::execute_ingest` records authoritative primary master path, category, size, capture date, BLAKE3 hash, and `CanonicalMaster` status on all new master documents in `src/application/ingest_service.rs`
- [X] T008 [US1] Verify that failed file copies or integrity mismatches abort master registration without leaving orphaned catalog records in `src/application/ingest_service.rs`

**Checkpoint**: User Story 1 complete and independently verifiable.

---

## Phase 4: User Story 2 - Comprehensive Master Linking for All Duplicate Copies (Priority: P1)

**Goal**: When ingesting from a folder with multiple copies of the same file under different names, exactly one becomes `CanonicalMaster` on the primary storage and all remaining duplicate copies across MongoDB are marked `MarkForRemoval` and linked to the master ID and path.

**Independent Test**: Ingesting a cluster of 5 identical files creates 1 `CanonicalMaster` document and updates all 4 sibling records in MongoDB to `MarkForRemoval` with `canonical_master_id` and `canonical_master_path`. Duplicate pruning then verifies and deletes all 4 redundant copies without warnings.

### Tests for User Story 2
- [X] T009 [P] [US2] Add unit test verifying that a 5-copy duplicate cluster has all copies linked to the master upon ingest in `tests/test_master_ingest.rs`

### Implementation for User Story 2
- [X] T010 [US2] Update `link_canonical_masters` in `src/application/ingest_service.rs` and `src/infrastructure/mongodb_sink.rs` to guarantee all duplicate records sharing the content hash have their master ID and master path populated
- [X] T011 [US2] Verify `DrivePruningService::generate_manifest` and execution safely verifies and deletes all linked duplicates in `src/application/drive_pruner.rs`

**Checkpoint**: User Story 2 complete and independently verifiable.

---

## Phase 5: User Story 3 - Replica Synchronization State Tracking (Priority: P2)

**Goal**: Track replication lifecycle state on every canonical master document (`Pending`, `PartiallyReplicated`, `Replicated`) across configured backup drives.

**Independent Test**: Ingest an asset with replica drives configured; verify master has `replication.status = Pending` with all replica drive IDs. Run replica sync to one replica drive; verify status becomes `PartiallyReplicated`. Sync to all replica drives; verify status becomes `Replicated`.

### Tests for User Story 3
- [X] T012 [P] [US3] Add integration test for master replication state lifecycle (`Pending` -> `PartiallyReplicated` -> `Replicated`) in `tests/test_replica_sync.rs`

### Implementation for User Story 3
- [X] T013 [US3] In `src/application/ingest_service.rs`, discover configured replica targets from `DriveConfiguration` and initialize `MasterReplicationState` with pending replica drives on newly created master documents
- [X] T014 [US3] In `src/application/replica_service.rs`, update `MultiReplicaSyncService::execute_sync` to update the master document's `MasterReplicationState` (moving synchronized drive IDs to `replicated_drives`) and transitioning `replication.status`

**Checkpoint**: User Story 3 complete and independently verifiable.

---

## Phase 6: User Story 4 - Seamless 3-Step CLI Workflow & Status Feedback (Priority: P3)

**Goal**: Provide clear, transparent terminal feedback for the 3-step workflow detailing masters registered, duplicates linked, and pending replica counts.

**Independent Test**: Run the 3-step sequence from the CLI and verify stdout/stderr reflects exact counts of masters created, duplicates linked, and pending replica assets.

### Implementation for User Story 4
- [X] T015 [US4] Add `duplicates_linked` and `replication_pending_count` to `IngestExecutionReport` in `src/domain/dedup_models.rs` and format console output in `src/main.rs`
- [X] T016 [US4] Update `sync-replicas` command output in `src/main.rs` to show updated master replication counts

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: End-to-end validation, performance, and code quality

- [X] T017 [P] Validate complete end-to-end scenario matching `quickstart.md` in `tests/test_master_ingest.rs`
- [X] T018 Run `cargo fmt` and `cargo clippy --all-targets -- -D warnings` to verify zero compiler warnings and clean formatting

---

## Dependencies & Execution Order

- **Phase 1 (Setup)**: T001
- **Phase 2 (Foundational)**: T002, T003, T004, T005 (Blocks US1, US2, US3)
- **Phase 3 (US1 - MVP)**: T006, T007, T008 (Can be implemented and tested independently)
- **Phase 4 (US2)**: T009, T010, T011 (Depends on US1 master creation)
- **Phase 5 (US3)**: T012, T013, T014 (Depends on US1 master registration & foundational storage methods)
- **Phase 6 (US4)**: T015, T016 (Depends on US1, US2, US3 report fields)
- **Phase 7 (Polish)**: T017, T018 (Final end-to-end verification and linting)
