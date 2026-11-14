---
description: "Task list for Primary and Replica Archive Immutability & Deletion Protection"
---

# Tasks: Primary and Replica Archive Immutability & Deletion Protection

**Input**: Design documents from `specs/014-protect-primary-replicas/`
**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/cli-contract.md](contracts/cli-contract.md)

**Tests**: Tests are included to verify critical archive protection guarantees across all stages.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3, US4)
- Exact file paths are included in all descriptions.

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Define error types and configuration models for archive immutability.

- [x] T001 [P] Add `DeduplicationError::PruneForbiddenOnProtectedArchive` variant with error message in `src/domain/errors.rs`
- [x] T002 [P] Verify `DriveRules` in `src/domain/dedup_models.rs` has `allow_pruning` and default helper methods if needed

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core protection guard component that all user stories depend on.

**⚠️ CRITICAL**: Must be completed before user story integration begins.

- [x] T003 Implement `ArchiveProtectionGuard` in `src/infrastructure/archive_guard.rs` with `from_config`, `is_protected_path`, `canonicalize_safe`, and `safe_delete_file` methods
- [x] T004 Expose `archive_guard` module in `src/infrastructure/mod.rs`
- [x] T005 [P] Unit test `ArchiveProtectionGuard` path canonicalization and protection rules in `src/infrastructure/archive_guard.rs`

**Checkpoint**: Foundation ready - `ArchiveProtectionGuard` is unit-tested and ready for integration.

---

## Phase 3: User Story 1 - Immutable Archive Guard for Primary and Replica Storage (Priority: P1) 🎯 MVP

**Goal**: Guarantee that any file on primary master or replica paths can never be deleted by pruning, manifest generation, or runtime file system operations.

**Independent Test**: Attempt to delete or generate manifests containing files on primary/replica paths. Verify physical files are retained and errors are recorded.

### Tests for User Story 1

- [x] T006 [P] [US1] Integration test verifying candidate disqualification when physical file path resolves to a primary or replica drive in `tests/test_drive_pruner.rs`
- [x] T007 [P] [US1] Integration test verifying runtime circuit breaker prevents file deletion on protected archives in `tests/test_drive_pruner.rs`

### Implementation for User Story 1

- [x] T008 [US1] Integrate `ArchiveProtectionGuard` into `DrivePruningService` struct in `src/application/drive_pruner.rs`
- [x] T009 [US1] Update `DrivePruningService::generate_manifest` in `src/application/drive_pruner.rs` to filter out any candidates residing within protected archive roots
- [x] T010 [US1] Update `DrivePruningService::prune_drive` in `src/application/drive_pruner.rs` to replace direct `std::fs::remove_file` with `ArchiveProtectionGuard::safe_delete_file`

**Checkpoint**: User Story 1 complete. Primary and replica paths cannot be deleted or queued for deletion during pruning operations.

---

## Phase 4: User Story 2 - Fail-Fast Protection for Drive Pruning Commands (Priority: P1)

**Goal**: Immediately abort `dedup prune` execution if the operator passes a drive ID or path corresponding to a Primary Master or Replica drive.

**Independent Test**: Execute `dedup prune` targeting primary or replica drives; command aborts immediately with exit code and explicit error message without database scanning or file changes.

### Tests for User Story 2

- [x] T011 [P] [US2] Integration test for fail-fast rejection of primary and replica drive IDs in `tests/test_drive_pruner.rs`

### Implementation for User Story 2

- [x] T012 [US2] Add `DrivePruningService::validate_prune_target` in `src/application/drive_pruner.rs` checking drive group (`primary` or replica) and resolving target path
- [x] T013 [US2] Call `validate_prune_target` at the start of both `generate_manifest` and `prune_drive` in `src/application/drive_pruner.rs` and CLI wiring in `src/main.rs`

**Checkpoint**: User Story 2 complete. Accidental pruning invocations against primary or replica volumes fail fast before any processing.

---

## Phase 5: User Story 3 - Ingestion Source Deletion Circuit Breaker (`--delete-source`) (Priority: P2)

**Goal**: Prevent source file deletion during `ingest --delete-source` if the source file is located on a Primary Master or Replica volume.

**Independent Test**: Run `ingest --delete-source` pointing to a primary or replica source path; files are copied/verified but the source file is never deleted, logging a protection warning.

### Tests for User Story 3

- [x] T014 [P] [US3] Integration test verifying `--delete-source` never deletes source files when source path is on primary or replica drive in `tests/test_master_ingest.rs`

### Implementation for User Story 3

- [x] T015 [US3] Integrate `ArchiveProtectionGuard` into `MasterIngestService` in `src/application/ingest_service.rs`
- [x] T016 [US3] Replace raw `std::fs::remove_file(&task.source_path)` in `MasterIngestService::execute_ingest` with `ArchiveProtectionGuard::safe_delete_file` with warning telemetry

**Checkpoint**: User Story 3 complete. Ingestion source deletion cannot delete primary or replica assets.

---

## Phase 6: User Story 4 - Canonicalized Symlink and Traversal Protection (Priority: P3)

**Goal**: Ensure symlinks, relative path segments (`..`), and aliased paths cannot bypass archive immutability checks.

**Independent Test**: Create symlinks pointing to primary/replica files and verify that pruning and ingest circuit breakers resolve canonical target and block deletion.

### Tests for User Story 4

- [x] T017 [P] [US4] Integration test verifying symlinks pointing to protected primary/replica paths are blocked from deletion in `tests/test_drive_pruner.rs`

### Implementation for User Story 4

- [x] T018 [US4] Enhance `ArchiveProtectionGuard::canonicalize_safe` in `src/infrastructure/archive_guard.rs` to handle symlinks pointing to protected directories and files, dereferencing all symlink hops before prefix validation

**Checkpoint**: User Story 4 complete. Symlinks and path traversals cannot circumvent archive immutability.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: End-to-end verification, documentation, clippy and formatting checks.

- [x] T019 Run full test suite (`cargo test`) to ensure zero regression across all 111+ existing tests and new safety tests
- [x] T020 Run `cargo clippy --all-targets -- -D warnings` and fix any linter warnings
- [x] T021 Run `cargo fmt --check` to ensure code conforms to repository standards
- [x] T022 Validate end-to-end against `specs/014-protect-primary-replicas/quickstart.md` scenarios

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies - can start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 completion - BLOCKS all user stories
- **User Story 1 (Phase 3)**: Depends on Phase 2 - Core pruning protection
- **User Story 2 (Phase 4)**: Depends on Phase 2 & 3 - Fail-fast CLI validation
- **User Story 3 (Phase 5)**: Depends on Phase 2 - Ingest source deletion circuit breaker
- **User Story 4 (Phase 6)**: Depends on Phase 2, 3, 5 - Deep symlink resolution
- **Polish (Phase 7)**: Depends on all user stories completed

### User Story Dependencies

- **US1 (P1)**: Core guard & pruning disqualification.
- **US2 (P1)**: Pruning command fail-fast. Extends `drive_pruner.rs`.
- **US3 (P2)**: Ingest `--delete-source` circuit breaker. Modifies `ingest_service.rs`.
- **US4 (P3)**: Symlink canonicalization resilience in `archive_guard.rs`.

---

## Parallel Opportunities

- T001 and T002 can run in parallel (different files in `src/domain/`).
- T006, T007, T011, T014, T017 test creation can be authored in parallel with implementation.
- US3 (`ingest_service.rs`) can be developed in parallel with US1/US2 (`drive_pruner.rs`) once Phase 2 is complete.

---

## Implementation Strategy

### MVP First (User Story 1 & 2)

1. Complete Phase 1 (Setup) and Phase 2 (Foundational `ArchiveProtectionGuard`).
2. Complete Phase 3 (US1 Pruning candidate filtering & runtime guard) and Phase 4 (US2 Fail-fast abort).
3. Validate: `cargo test --test test_drive_pruner`.

### Incremental Delivery

1. Foundation ready (`ArchiveProtectionGuard`).
2. Deliver US1 + US2: Pruning protection complete.
3. Deliver US3: Ingest protection complete.
4. Deliver US4: Symlink traversal defense verified.
5. Final polish: Full regression test, clippy, fmt.
