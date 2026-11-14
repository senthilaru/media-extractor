# Tasks: Apple AAE Sidecar File Support

**Input**: Design documents from `/specs/019-aae-sidecar-support/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/sidecar-service-contract.md

## Format: `- [ ] [TaskID] [P?] [Story?] Description with file path`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: User story label (`[US1]`, `[US2]`, `[US3]`, `[US4]`)

---

## Phase 1: Setup

**Purpose**: Register sidecar domain models and category representations

- [x] T001 Add `MediaCategory::Sidecar` variant and display formatting in `src/domain/models.rs`
- [x] T002 [P] Add `SidecarCompanionPair`, `SidecarTransferResult`, and `SidecarPruneResult` models in `src/domain/dedup_models.rs`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core sidecar classification and discovery mechanisms

- [x] T003 Register `"aae"` extension in `EXTENSION_MAP` in `src/infrastructure/classifier.rs` and update classifier tests
- [x] T004 Implement `find_companion_sidecar` and Apple edit variant resolver (`IMG_E*`) in `src/application/transfer_service.rs`

---

## Phase 3: User Story 1 - Automatic Ingestion of Companion AAE Sidecars Beside Images/Videos (Priority: P1) 🎯 MVP

**Goal**: Automatically ingest companion `.AAE` sidecar files into the canonical primary master directory alongside their corresponding photo/video.

**Independent Test**: Ingest an unorganized folder containing `IMG_1001.HEIC` and `IMG_1001.AAE`. Verify both files are transferred into the same date-based canonical folder in the primary master archive.

- [x] T005 [US1] Integrate companion `.AAE` sidecar transfer into `TransferService::execute_transfer` and `MasterIngestService::execute_ingest_plan` in `src/application/transfer_service.rs` and `src/application/ingest_service.rs`
- [x] T006 [P] [US1] Add integration test in `tests/test_aae_sidecars.rs` verifying `.AAE` files are ingested alongside photos and videos in canonical master folders

---

## Phase 4: User Story 2 - Parity Replication of AAE Sidecar Files (Priority: P2)

**Goal**: Replicate companion `.AAE` sidecar files from the canonical master archive to all active replica drives.

**Independent Test**: Run replica synchronization on a master directory containing `IMG_2000.JPG` and `IMG_2000.AAE`. Verify the replica drive receives both files at identical relative paths.

- [x] T007 [US2] Implement companion `.AAE` sidecar discovery and fanout copying in `ReplicaSyncService::execute_sync` in `src/application/replica_service.rs`
- [x] T008 [P] [US2] Add integration test in `tests/test_aae_sidecars.rs` verifying `.AAE` sidecars replicate to backup drives with hash verification

---

## Phase 5: User Story 3 - Clean Pruning of Ingested AAE Sidecar Files (Priority: P3)

**Goal**: Prune source `.AAE` sidecar files once verified in master, allowing empty source directories to be deleted.

**Independent Test**: Run pruning on an unorganized folder containing `IMG_0050.JPG` and `IMG_0050.AAE`. Verify both are pruned and the empty directory is deleted.

- [x] T009 [US3] Implement companion `.AAE` sidecar verification and pruning in `DrivePruningService::prune_drive` in `src/application/drive_pruner.rs`
- [x] T010 [P] [US3] Add integration test in `tests/test_aae_sidecars.rs` verifying source `.AAE` files are pruned and empty directories cleaned

---

## Phase 6: User Story 4 - Handling Orphan and Pre-Ingested AAE Sidecars (Priority: P4)

**Goal**: Safely handle `.AAE` files whose companion media file was already previously ingested or is absent.

**Independent Test**: Ingest an `.AAE` file whose matching image is already in master. Verify the `.AAE` is routed to the existing master folder without error.

- [x] T011 [US4] Implement catalog lookup routing for orphan or pre-ingested `.AAE` sidecars in `src/application/ingest_service.rs`

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Verification, formatting, and linting

- [x] T012 Run test suite `cargo test`, verify formatting `cargo fmt --check`, and verify lints `cargo clippy --all-targets -- -D warnings`
