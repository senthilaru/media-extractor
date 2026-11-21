# Tasks: Multi-Volume Storage Extension Pools

**Input**: Design documents from `/specs/024-storage-extension-pools/` (`spec.md`, `plan.md`, `research.md`, `data-model.md`, `contracts/storage-pool-contract.md`, `quickstart.md`)

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (`US1`, `US2`, `US3`, `US4`)
- Include exact file paths in descriptions

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Core interfaces and hardware probe primitives for storage pool operations

- [X] T001 [P] Create disk space probe abstraction `DiskSpaceProbe` trait and POSIX implementation `PosixDiskSpaceProbe` in `src/infrastructure/disk_probe.rs`
- [X] T002 Register `disk_probe` module in `src/infrastructure/mod.rs` and export probe traits

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core pool data models, drive configuration extensions, and env synthesis

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T003 [P] Add storage pool grouping helper methods (`get_pool_volumes`, `get_primary_pool`, `get_replica_pool`, `get_storage_groups`) to `DriveConfiguration` in `src/domain/dedup_models.rs`
- [X] T004 Add support for extension drive environment variables (`DRIVE_PRIMARY_EXT*`, `DRIVE_REPLICA*_EXT*`) and multi-volume pool parsing in `src/infrastructure/drives_config.rs`

**Checkpoint**: Foundation ready - user story implementation can begin

---

## Phase 3: User Story 1 - Storage Pool Definition & Single Canonical Copy Across Extension Volumes (Priority: P1) 🎯 MVP

**Goal**: Support multiple member volumes in the primary pool and replica groups, enforcing the single canonical copy invariant where duplicates across member volumes in the same pool are marked for removal.

**Independent Test**: Mount simulated primary extension drives with overlapping files; run deduplication analysis and verify exactly one file is marked `CanonicalMaster` (the one on the lower-index volume) while duplicates across extension drives are marked `MarkForRemoval`.

### Tests for User Story 1
- [X] T005 [P] [US1] Unit tests for `CanonicalPathMatcher` and `DriveConfiguration` multi-volume pool lookups in `tests/test_database_dedup.rs`
- [X] T006 [P] [US1] Integration tests in `tests/test_database_dedup.rs` verifying single-copy deduplication invariant across primary extension volumes and intra-replica extension duplicates

### Implementation for User Story 1
- [X] T007 [US1] Update `MediaDeduplicationEngine::evaluate_cluster` in `src/application/dedup_engine.rs` to select a single `CanonicalMaster` across all primary pool volumes (picking the first volume in declaration order) and mark other intra-primary copies as `MarkForRemoval` instead of creating an unresolved conflict
- [X] T008 [US1] Update `MediaDeduplicationEngine::evaluate_cluster` in `src/application/dedup_engine.rs` to enforce at most one `ReplicaKeep` per replica group across all its member volumes, marking intra-replica duplicates as `MarkForRemoval`

**Checkpoint**: User Story 1 is functional and verifiable independently.

---

## Phase 4: User Story 2 - Capacity-Aware Ingestion & Volume Spillover (Priority: P1)

**Goal**: Route newly ingested assets into the primary storage pool, filling the base volume first and automatically spilling over into extension volumes when free space falls below the 5 GB buffer.

**Independent Test**: Ingest assets into a mock primary pool where `primary_vol1` has less than 5 GB free space; verify new assets route to `primary_vol2`.

### Tests for User Story 2
- [X] T009 [P] [US2] Unit test for volume spillover selection logic with `MockDiskSpaceProbe` in `tests/test_master_ingest.rs`

### Implementation for User Story 2
- [X] T010 [US2] Implement `resolve_pool_target_volume` with `DiskSpaceProbe` and 5 GB threshold in `src/application/ingest_service.rs`
- [X] T011 [US2] Update `compile_plan` and `execute` in `src/application/ingest_service.rs` to dynamically assign target root based on capacity spillover

**Checkpoint**: User Story 2 allows overflow ingestion across multiple primary volumes seamlessly.

---

## Phase 5: User Story 3 - Multi-Volume Replica Synchronization (Priority: P2)

**Goal**: Mirror primary assets to replica pools, recognizing assets already existing on any volume in a replica group and spilling new replicas over to replica extension drives when needed.

**Independent Test**: Synchronize an asset to a replica pool containing `replica_1_vol1` and `replica_1_vol2`. If present on `replica_1_vol2`, verify no redundant copy is sent to `replica_1_vol1`. If `replica_1_vol1` is full, verify transfer spills to `replica_1_vol2`.

### Tests for User Story 3
- [X] T012 [P] [US3] Unit & integration tests in `tests/test_replica_sync.rs` for multi-volume replica presence detection and spillover transfer

### Implementation for User Story 3
- [X] T013 [US3] Implement `is_replicated_in_pool` across all member volumes of a replica group in `src/application/replica_service.rs`
- [X] T014 [US3] Update `compile_plan` and `execute` in `src/application/replica_service.rs` to support target pool spillover using `DiskSpaceProbe`

**Checkpoint**: Multi-volume replica pools are synchronized correctly without redundant copying across pool volumes.

---

## Phase 6: User Story 4 - Comprehensive Archive Protection Across All Pool Volumes (Priority: P2)

**Goal**: Guarantee all member volumes of primary and replica pools are strictly protected by `ArchiveProtectionGuard`, preventing any destructive prune operations against them.

**Independent Test**: Run prune against any member drive in a primary or replica extension pool; verify operation is rejected with an archive protection error.

### Tests for User Story 4
- [X] T015 [P] [US4] Integration test in `tests/test_drive_pruner.rs` confirming prune attempts on primary extension and replica extension drives fail with archive protection errors

### Implementation for User Story 4
- [X] T016 [US4] Verify and ensure `ArchiveProtectionGuard::from_config` in `src/infrastructure/archive_guard.rs` encompasses all member volumes across primary and replica pools

**Checkpoint**: Archive protection covers 100% of member volumes across all pools.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: End-to-end testing, documentation updates, and regression verification

- [X] T017 [P] Create end-to-end integration test `tests/test_storage_pools.rs` exercising multi-volume pool scan, deduplication, ingest spillover, replica sync, and prune protection
- [X] T018 [P] Update `drives.example.yaml`, `README.md`, and `apis.md` with multi-volume storage pool configuration examples
- [X] T019 Run full workspace test suite (`cargo test`) to ensure zero regressions across all existing features
- [X] T020 Enable offline master tolerance in `DrivePruningService` and `OrganizePipelineService` so duplicate pruning verifies against the database catalog without requiring disconnected master drives to be physically mounted
- [X] T021 Filter pool volumes in `MasterIngestService` and `PosixDiskSpaceProbe` to only consider mounted disks, preventing spurious probes or write attempts to detached volumes

---

## Dependencies & Execution Order

### Phase Dependencies
- **Setup (Phase 1)**: Can start immediately.
- **Foundational (Phase 2)**: Depends on Phase 1 completion. Blocks all User Stories.
- **User Stories (Phase 3+)**: Depend on Phase 2 completion.
  - US1 (P1 - Dedup) and US2 (P1 - Ingest) can proceed sequentially or in parallel.
  - US3 (P2 - Replica) depends on pool helpers and spillover concepts from US1/US2.
  - US4 (P2 - Protection) can run after Foundational.
- **Polish (Phase 7)**: Depends on all user stories completing.

### Implementation Strategy
1. **MVP First**: Complete Phase 1, Phase 2, and Phase 3 (US1). Validate single canonical copy across extension drives.
2. **Phase 4**: Enable capacity spillover for ingestion (US2).
3. **Phase 5**: Enable replica synchronization across pools (US3).
4. **Phase 6 & 7**: Enforce archive protection and run full end-to-end validation (US4, Polish).
