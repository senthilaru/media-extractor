# Tasks: Prune Empty Directories Cleanup

**Input**: Design documents from `/specs/017-prune-empty-directories/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/cli-prune-contract.md

## Format: `- [ ] [TaskID] [P?] [Story?] Description with file path`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: User story label (`[US1]`, `[US2]`, `[US3]`)

---

## Phase 1: Setup

**Purpose**: Baseline preparation and test harness verification

- [X] T001 Verify existing prune test harness in `tests/test_drive_pruner.rs`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Data model and core helper functions required by all user stories

- [X] T002 Add `empty_directories_removed` and `empty_directories_eligible` to `PruneExecutionReport` in `src/application/drive_pruner.rs`
- [X] T003 Implement `is_disposable_os_file` helper for `.DS_Store` and `Thumbs.db` detection in `src/application/drive_pruner.rs`

---

## Phase 3: User Story 1 - Automatic Empty Directory Removal on Prune Execution (Priority: P1) 🎯 MVP

**Goal**: Automatically delete subdirectories under the prune target that become empty after duplicate files are deleted.

**Independent Test**: Execute prune on a directory structure containing duplicates; verify empty subdirectories are removed while directories containing retained files remain.

- [X] T004 [US1] Implement `clean_empty_ancestor_directories` upward traversal with depth-descending sorting in `src/application/drive_pruner.rs`
- [X] T005 [US1] Integrate empty directory cleanup into `DrivePruningService::prune_drive` after file deletions in `src/application/drive_pruner.rs`
- [X] T006 [P] [US1] Add unit tests in `tests/test_drive_pruner.rs` verifying empty directory cleanup, retention of non-empty folders, and `.DS_Store` handling

---

## Phase 4: User Story 2 - Boundary Preservation of Target and Protected Roots (Priority: P2)

**Goal**: Guarantee that the user-specified target root folder and protected canonical master archives are never deleted.

**Independent Test**: Target a directory where all contents are duplicates; verify that child subdirectories are deleted but the target root itself remains on disk.

- [X] T007 [US2] Enforce boundary check against prune target root and `ArchiveProtectionGuard` in `src/application/drive_pruner.rs`
- [X] T008 [P] [US2] Add unit test in `tests/test_drive_pruner.rs` verifying target root is never deleted even when emptied

---

## Phase 5: User Story 3 - Dry-Run Preview and Telemetry Reporting (Priority: P3)

**Goal**: Surface empty directory counts in both dry-run preview and execution telemetry.

**Independent Test**: Run prune in preview mode and verify dry-run outputs eligible empty directory count without deleting folders.

- [X] T009 [US3] Implement dry-run simulation of empty directory count in `src/application/drive_pruner.rs`
- [X] T010 [US3] Update CLI prune terminal presentation in `src/main.rs` to show empty directories in dry-run and execution summaries
- [X] T011 [P] [US3] Add unit test in `tests/test_drive_pruner.rs` verifying dry-run reporting and zero filesystem modifications

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Validation, formatting, and test suite execution

- [X] T012 Run full test suite `cargo test` and verify `cargo fmt --check` and `cargo clippy`

---

## Phase 7: Upward Directory Pruning to Drive Mount Boundary for Organize

**Purpose**: Enable automated directory cleanup in the organize pipeline to remove empty source folders and parents up to drive mount boundary ceiling

- [X] T013 Implement `resolve_drive_boundary_ceiling` and `is_system_or_home_root` in `src/application/drive_pruner.rs`
- [X] T014 Enhance `clean_empty_ancestor_directories_with_options` supporting `boundary_ceiling` and `preserve_prune_root` in `src/application/drive_pruner.rs`
- [X] T015 Update `OrganizeService` Stage 5 to pass drive ceiling and prune empty source folder hierarchy by default unless `--keep-source-dir` is passed in `src/application/organize_service.rs`
- [X] T016 Add integration test in `tests/test_organize_pipeline.rs` verifying cleanup of source folder and parent folders up to drive ceiling

---

## Phase 8: Zero-Byte Dummy File Disposal & Database Purging

**Purpose**: Recognize 0-byte corrupt/dummy files as disposable so ghost directory trees can be unlinked and purged

- [X] T017 Implement `is_disposable_file` recognizing OS files and 0-byte files in `src/application/drive_pruner.rs`
- [X] T018 Update `check_directory_empty` and dry-run simulation to unlink 0-byte files in `src/application/drive_pruner.rs`
- [X] T019 Add `purge_zero_byte_records` to `MetadataStorageSink` trait in `src/domain/ports.rs` and implement in `src/infrastructure/mongodb_sink.rs`
- [X] T020 Add `--clean-empty-dirs` option to `PruneArgs` and `DrivePrunerOptions` for on-demand drive sweeping in `src/main.rs` and `src/application/drive_pruner.rs`
- [X] T021 Add unit test in `tests/test_drive_pruner.rs` verifying 0-byte file unlinking, empty directory removal, and root preservation

