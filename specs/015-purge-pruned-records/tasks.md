---
description: "Task list for Purge Obsolete Database Records on Duplicate Prune"
---

# Tasks: Purge Obsolete Database Records on Duplicate Prune

**Input**: Design documents from `specs/015-purge-pruned-records/`
**Prerequisites**: [plan.md](plan.md), [spec.md](spec.md), [research.md](research.md), [data-model.md](data-model.md), [contracts/cli-contract.md](contracts/cli-contract.md)

**Tests**: Integration tests are included to verify that database records are permanently deleted when duplicates are pruned and canonical masters remain untouched.

**Organization**: Tasks are grouped by user story to enable independent implementation and testing of each story.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)
- Include exact file paths in descriptions.

---

## Phase 1: Setup

**Purpose**: Update execution models to support tracking purged records.

- [x] T001 Update `PruneExecutionReport` struct in `src/application/drive_pruner.rs` to include `pub records_purged: usize`

---

## Phase 2: Foundational (Storage Ports & Infrastructure)

**Purpose**: Core storage methods for hard deletion of metadata documents.

**⚠️ CRITICAL**: Must be completed before user story implementation can begin.

- [x] T002 Add `delete_document`, `delete_documents`, and `purge_removed_records` methods to `MetadataStorageSink` trait in `src/domain/ports.rs`
- [x] T003 Implement `delete_document`, `delete_documents`, and `purge_removed_records` in `src/infrastructure/mongodb_sink.rs` with safety checks against deleting `CanonicalMaster` or `ReplicaKeep` records
- [x] T004 Update mock storage sinks in `tests/test_drive_pruner.rs`, `tests/test_master_ingest.rs`, `tests/test_database_dedup.rs`, and `tests/test_replica_sync.rs` to support `delete_document`, `delete_documents`, and `purge_removed_records`

**Checkpoint**: Foundation ready - storage sinks support document deletion.

---

## Phase 3: User Story 1 - Purge Database Records for Pruned Duplicate Files (Priority: P1) 🎯 MVP

**Goal**: Permanently delete database records for duplicate files pruned during `dedup prune --execute`.

**Independent Test**: Run `dedup prune --execute` against duplicate candidates; verify files are deleted from disk and querying database returns 0 records for those document IDs while master records remain untouched.

### Implementation for User Story 1

- [x] T005 [US1] Update `DrivePruningService::prune_drive` in `src/application/drive_pruner.rs` to call `sink.delete_document(task.document_id)` instead of `sink.mark_removed(task.document_id)` and increment `records_purged`
- [x] T006 [US1] Integration test in `tests/test_drive_pruner.rs` verifying that pruned files have their database records hard-deleted and master documents remain intact

**Checkpoint**: User Story 1 complete. Pruning duplicate files deletes their database records.

---

## Phase 4: User Story 2 - Purge Pre-Existing "Removed" Records on Target Drive (Priority: P2)

**Goal**: Automatically purge historical records on the target drive that currently have status `Removed`.

**Independent Test**: Insert records with status `Removed` on the target drive; run `dedup prune --execute`; verify those records are deleted and counted in `records_purged`.

### Implementation for User Story 2

- [x] T007 [US2] Update `DrivePruningService::prune_drive` in `src/application/drive_pruner.rs` to invoke `sink.purge_removed_records(Some(&mount_str))` during `--execute` and add purged count to `records_purged`
- [x] T008 [US2] Integration test in `tests/test_drive_pruner.rs` verifying pre-existing `Removed` records are purged on prune execute

**Checkpoint**: User Story 2 complete. Historical `Removed` records are purged.

---

## Phase 5: User Story 3 - Telemetry & Audit Visibility for Purged Records (Priority: P3)

**Goal**: Provide clear console and report metrics showing `Database Records Purged: N` for `--execute` and `Database Records to Purge: N` for dry-run preview.

**Independent Test**: Execute dry-run and live prune runs; verify console output formats match CLI contract.

### Implementation for User Story 3

- [x] T009 [US3] Update `run_dedup_prune` in `src/main.rs` to display `Database Records to Purge` in dry-run output and `Database Records Purged` in execution output

**Checkpoint**: User Story 3 complete. Telemetry is visible to operators.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Test suite regression check, linter, formatting.

- [x] T010 Run full test suite (`cargo test`) to ensure zero regressions across all tests
- [x] T011 Run `cargo clippy --all-targets -- -D warnings`
- [x] T012 Run `cargo fmt --check`

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies
- **Foundational (Phase 2)**: Depends on Phase 1 - BLOCKS user stories
- **User Story 1 (Phase 3)**: Depends on Phase 2
- **User Story 2 (Phase 4)**: Depends on Phase 2 & 3
- **User Story 3 (Phase 5)**: Depends on Phase 3 & 4
- **Polish (Phase 6)**: Depends on all user stories completed
