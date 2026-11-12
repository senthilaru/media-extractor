# Tasks: Automated Master Ingestion and Source Deletion Marking

**Input**: Design documents from `/specs/010-ingest-to-master/`
**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md, data-model.md, contracts/

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (`[US1]`, `[US2]`, `[US3]`, `[US4]`)
- Exact file paths included in every task

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Shared domain models and error variants required by all user stories

- [X] T001 [P] Define `IngestTask`, `IngestTaskStatus`, `MasterIngestPlan`, `IngestOptions`, and `IngestExecutionReport` domain structs in src/domain/dedup_models.rs
- [X] T002 [P] Add ingestion error variants (`TransferFailed`, `IntegrityMismatch`, `InsufficientTargetSpace`, `DriveNotMounted`) to `DeduplicationError` in src/domain/errors.rs

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core port trait definitions and database sink query methods

**⚠️ CRITICAL**: Foundational tasks must complete before user story implementation

- [X] T003 Add `query_unmastered_candidates` to `MetadataStorageSink` trait in src/domain/ports.rs
- [X] T004 Implement `query_unmastered_candidates` in src/infrastructure/mongodb_sink.rs matching `dedup.status == "NeedsIngestToMaster"`

**Checkpoint**: Core domain models and storage ports ready.

---

## Phase 3: User Story 1 - Automated Ingestion of Unmastered Assets to Primary Master (Priority: P1) 🎯 MVP

**Goal**: Read unmastered files on a secondary drive, generate canonical destination paths (`<MasterRoot>/<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}`), stream copy files, register new `CanonicalMaster` records, and update source records to `MarkForRemoval`.

**Independent Test**: Mount mock secondary drive with unmastered files, run ingest, verify files exist in canonical hierarchy on primary drive, and verify database statuses are updated.

- [X] T005 [US1] Implement canonical destination path resolver and collision suffix handler in src/application/ingest_service.rs
- [X] T006 [US1] Implement chunked streaming file copier using 1MB bounded buffer in src/application/ingest_service.rs
- [X] T007 [US1] Implement master database registration and source `MarkForRemoval` update in src/application/ingest_service.rs
- [X] T008 [P] [US1] Integration tests for core ingest workflow and folder structure in tests/test_master_ingest.rs

**Checkpoint**: User Story 1 functional as an MVP; files safely copied to primary master and source marked for removal.

---

## Phase 4: User Story 2 - Bitwise Verification & Safe Master Registration (Priority: P2)

**Goal**: Perform post-copy size and BLAKE3 cryptographic hash verification before updating MongoDB or marking source records.

**Independent Test**: Simulate truncated or corrupted file transfer; verify that the system cleans up the corrupt destination file, leaves the source document as `NeedsIngestToMaster`, and logs a descriptive error.

- [X] T009 [US2] Implement BLAKE3 cryptographic verification comparing destination digest against source fingerprint in src/application/ingest_service.rs
- [X] T010 [US2] Implement corrupted copy rollback and failure isolation in src/application/ingest_service.rs
- [X] T011 [P] [US2] Integration test verifying rollback on corrupt transfer and zero data loss in tests/test_master_ingest.rs

---

## Phase 5: User Story 3 - Ingest Preview & Dry-Run Planning (Priority: P2)

**Goal**: Compile and output a preview table showing all planned file transfers, dates, destination paths, and total gigabytes without modifying disk or database state.

**Independent Test**: Run ingest with `--dry-run` and verify plan output table matches expectation with zero file system or database mutations.

- [X] T012 [US3] Implement plan compilation and free space check in src/application/ingest_service.rs
- [X] T013 [US3] Implement dry-run preview formatting and terminal summary in src/main.rs
- [X] T014 [P] [US3] Unit and integration test for dry-run plan compilation in tests/test_master_ingest.rs

---

## Phase 6: User Story 4 - Post-Ingest Automatic Source Deletion Option (Priority: P3)

**Goal**: Provide `--delete-source` flag to physically delete verified source files immediately and update database status to `Removed`.

**Independent Test**: Run ingest with `--delete-source`; verify verified source files are unlinked from the secondary drive and their database records set to `Removed`.

- [X] T015 [US4] Implement immediate source file deletion and `Removed` database status update in src/application/ingest_service.rs
- [X] T016 [P] [US4] Integration test for `--delete-source` execution in tests/test_master_ingest.rs

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: CLI wiring, library exports, documentation, and quality gates

- [X] T017 Add `ingest` CLI subcommand and arguments (`--source-drive`, `--target-drive`, `--dry-run`, `--delete-source`) in src/main.rs
- [X] T018 Export new types (`MasterIngestService`, `MasterIngestPlan`, `IngestTask`, `IngestExecutionReport`) in src/lib.rs and src/application/mod.rs
- [X] T019 Run quality checks with `cargo clippy --all-targets -- -D warnings` and `cargo fmt --check`
- [X] T020 Run complete test suite with `cargo test`

---

## Dependencies & Execution Order

- **Phase 1 (Setup)** $\rightarrow$ **Phase 2 (Foundational)**: Blocks all stories.
- **Phase 2** $\rightarrow$ **Phase 3 (US1: Core Ingest Service & DB Update)**.
- **Phase 3** $\rightarrow$ **Phase 4 (US2: Bitwise Integrity Check & Rollback)**.
- **Phase 4** $\rightarrow$ **Phase 5 (US3: Dry-Run Preview)**.
- **Phase 5** $\rightarrow$ **Phase 6 (US4: Delete Source Option)**.
- **Phase 6** $\rightarrow$ **Phase 7 (Polish & Verification)**.
