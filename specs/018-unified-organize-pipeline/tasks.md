# Tasks: Unified Organize Pipeline Subcommand

**Input**: Design documents from `/specs/018-unified-organize-pipeline/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/cli-organize-contract.md

## Format: `- [ ] [TaskID] [P?] [Story?] Description with file path`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: User story label (`[US1]`, `[US2]`, `[US3]`)

---

## Phase 1: Setup

**Purpose**: Register module structure

- [x] T001 Register `organize_service` module in `src/application/mod.rs`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Data types and reporting models

- [x] T002 Implement `OrganizePipelineOptions` and `OrganizePipelineReport` in `src/application/organize_service.rs`

---

## Phase 3: User Story 1 - Full End-to-End Media Organization in a Single Command (Priority: P1) 🎯 MVP

**Goal**: Coordinate all 5 stages (Extract -> Dedup -> Ingest -> Replicate -> Prune) with progress telemetry.

**Independent Test**: Run `cargo run -- organize --source <DIR>` and verify files are extracted, audited, ingested, replicated, and pruned in one command.

- [x] T003 [US1] Implement `OrganizePipelineService` in `src/application/organize_service.rs` coordinating extraction, universal classification, master ingest, replica sync, and prune
- [x] T004 [US1] Add `OrganizeArgs` subcommand and `run_organize_cli` handler in `src/main.rs`
- [x] T005 [P] [US1] Add end-to-end integration test in `tests/test_organize_pipeline.rs` verifying the complete 5-stage workflow

---

## Phase 4: User Story 2 - Comprehensive Dry-Run Safety Preview (Priority: P2)

**Goal**: Support `--dry-run` to preview all 5 stages without modifying files on disk.

**Independent Test**: Run `cargo run -- organize --source <DIR> --dry-run` and verify preview report without filesystem alterations.

- [x] T006 [US2] Implement `--dry-run` simulation mode across all pipeline stages in `src/application/organize_service.rs`
- [x] T007 [P] [US2] Add test in `tests/test_organize_pipeline.rs` verifying zero modifications in dry-run mode

---

## Phase 5: User Story 3 - Selective Stage Flags (Priority: P3)

**Goal**: Support `--skip-prune` to ingest and replicate without removing source files.

**Independent Test**: Run `cargo run -- organize --source <DIR> --skip-prune` and verify source files remain intact.

- [x] T008 [US3] Add `--skip-prune` flag handling in `src/application/organize_service.rs` and `src/main.rs`
- [x] T009 [P] [US3] Add test in `tests/test_organize_pipeline.rs` verifying `--skip-prune` bypasses Stage 5

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Verification and code quality

- [x] T010 Run test suite `cargo test`, verify formatting `cargo fmt --check`, and verify lints `cargo clippy`

---

## Phase 7: Empty Source & Ancestor Directory Pruning to Drive Mount Ceiling

**Purpose**: Automatically remove vacated source folders and empty ancestors up to the drive mount ceiling in Stage 5, with `--keep-source-dir` opt-out.

- [x] T011 Add `--keep-source-dir` flag to `OrganizeArgs` and `OrganizePipelineOptions` in `src/main.rs` and `src/application/organize_service.rs`
- [x] T012 Configure Stage 5 prune to resolve `boundary_ceiling` and clean empty ancestor directories up to the drive mount point
- [x] T013 Add integration tests in `tests/test_organize_pipeline.rs` verifying empty source & ancestor directory removal up to drive mount ceiling and `--keep-source-dir` behavior
