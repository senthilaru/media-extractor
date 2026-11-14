# Tasks: External HDD I/O Transfer & Buffer Optimization

**Input**: Design documents from `specs/016-optimize-hdd-transfer/`

**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3, US4)

---

## Phase 1: Setup

**Purpose**: Define configuration models and value objects

- [X] T001 [P] Define `TransferBufferConfig` with bounds validation (`min(file_size, max_buffer)` between 64KB and 32MB) and human-readable string parsing (`4M`, `8M`, `16M`, `32M`) in `src/domain/dedup_models.rs`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core stream hashing and spindle synchronization infrastructure

- [X] T002 [P] Implement `InFlightStreamHasher` with incremental `blake3::Hasher` updates and verification in `src/application/transfer_service.rs`
- [X] T003 Implement `DriveSpindleLockManager` for per-mount spindle synchronization in `src/infrastructure/drive_spindle_lock.rs` and export in `src/infrastructure/mod.rs`

---

## Phase 3: User Story 1 - Tuned Chunk Sequential Streaming (Priority: P1) 🎯 MVP

**Goal**: Maximize contiguous sequential HDD throughput using 8 MB chunk buffers and dynamic small-file allocation

**Independent Test**: Transfer large files with 8 MB buffers and verify contiguous sustained sequential throughput without buffer starvation

- [X] T004 [P] [US1] Create unit tests for 8 MB buffer streaming and dynamic small-file buffer bounding in `tests/test_transfer_service.rs`
- [X] T005 [US1] Refactor `UnifiedFileTransferService::stream_copy_verified` to use `TransferBufferConfig` (default 8 MB), dynamic allocation $\min(\text{expected\_size}, \text{buffer\_size})$, and storage commit `dst_file.sync_all()` in `src/application/transfer_service.rs`

---

## Phase 4: User Story 2 - In-Flight Streaming Integrity Verification (Priority: P2)

**Goal**: Eliminate the secondary destination disk read pass by computing BLAKE3 cryptographic hash in-flight during stream copy

**Independent Test**: Verify that destination file cryptographic hash is verified against expected digest without secondary disk re-read

- [X] T006 [P] [US2] Create unit tests for in-flight hash computation and target file rollback upon mismatch in `tests/test_transfer_service.rs`
- [X] T007 [US2] Update `UnifiedFileTransferService::stream_copy_verified` to compute BLAKE3 hash in-flight during the chunk loop and verify against expected hash, skipping destination re-read unless `recheck_disk_hash` is requested in `src/application/transfer_service.rs`

---

## Phase 5: User Story 3 - Per-Drive Spindle Serialization (Priority: P3)

**Goal**: Prevent head seek thrashing on mechanical drives by serializing access per physical spindle

**Independent Test**: Verify that concurrent transfer tasks targeting the same mount point execute sequentially without interleaved head thrashing

- [X] T008 [P] [US3] Create tests verifying physical spindle lock serialization in `tests/test_drive_spindle_lock.rs`
- [X] T009 [US3] Integrate `DriveSpindleLockManager` into `MasterIngestService` and `ReplicaSyncService` to serialize task execution per physical drive in `src/application/ingest_service.rs` and `src/application/replica_service.rs`

---

## Phase 6: User Story 4 - Configurable Buffer Options & CLI Integration (Priority: P4)

**Goal**: Allow operators to configure buffer size (`--buffer-size`) and force disk re-read (`--recheck-disk-hash`) from the CLI

**Independent Test**: Run `ingest` and `sync-replicas` with `--buffer-size 16M` and `--recheck-disk-hash` and verify parameters are applied

- [X] T010 [P] [US4] Add `--buffer-size` and `--recheck-disk-hash` arguments to `IngestArgs` and `SyncReplicasArgs` in `src/main.rs`
- [X] T011 [US4] Propagate `TransferBufferConfig` from CLI arguments into `MasterIngestService` and `ReplicaSyncService` in `src/main.rs`

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Comprehensive regression validation, formatting, and linting

- [X] T012 [P] Run unit and integration tests across `test_master_ingest.rs`, `test_replica_sync.rs`, and `test_transfer_service.rs`
- [X] T013 Verify code formatting with `cargo fmt --check` and linting with `cargo clippy --all-targets -- -D warnings`

---

## Dependencies & Execution Order

- **Phase 1 (Setup)**: Can start immediately.
- **Phase 2 (Foundational)**: Depends on Phase 1 completion.
- **Phase 3 (User Story 1 - P1)**: Depends on Phase 2; establishes 8 MB streaming copy.
- **Phase 4 (User Story 2 - P2)**: Depends on Phase 3; integrates in-flight hashing into the 8 MB streaming copy.
- **Phase 5 (User Story 3 - P3)**: Depends on Phase 4; wraps stream transfers with spindle lock synchronization.
- **Phase 6 (User Story 4 - P4)**: Depends on Phase 5; exposes CLI parameters and plumbing.
- **Phase 7 (Polish)**: Runs after all user stories are implemented.
