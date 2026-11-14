# Tasks: Database Content & Health Audit Report

**Input**: Design documents from `/specs/020-database-content-report/`  
**Prerequisites**: [plan.md](file:///Users/spsenthil/Documents/repo/media/media-extractor/specs/020-database-content-report/plan.md), [spec.md](file:///Users/spsenthil/Documents/repo/media/media-extractor/specs/020-database-content-report/spec.md), [research.md](file:///Users/spsenthil/Documents/repo/media/media-extractor/specs/020-database-content-report/research.md), [data-model.md](file:///Users/spsenthil/Documents/repo/media/media-extractor/specs/020-database-content-report/data-model.md), [contracts/](file:///Users/spsenthil/Documents/repo/media/media-extractor/specs/020-database-content-report/contracts/)

## Format: `[TaskID] [P?] [Story?] Description with file path`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (`US1`, `US2`, `US3`)
- Exact file paths included in all descriptions

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Module declarations and shared project structure

- [x] T001 Register `audit_models` module in `src/domain/mod.rs` and `audit_service` module in `src/application/mod.rs`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core domain entities and database port abstractions that all user stories depend on

**⚠️ CRITICAL**: Must be completed before implementing user stories

- [x] T002 [P] Define `CatalogAuditReport`, `ExtensionCount`, `InspectionCategory`, `InspectionSample`, and `CommandRecommendation` entities in `src/domain/audit_models.rs`
- [x] T003 [P] Extend `StorageSink` trait in `src/domain/ports.rs` with `generate_audit_report` and `query_inspection_samples` methods
- [x] T004 Implement server-side MongoDB aggregation pipeline (`$facet` for status tallies and extension grouping) and sample query methods in `src/infrastructure/mongodb_sink.rs`

**Checkpoint**: Core domain models, port interfaces, and MongoDB aggregation engine ready.

---

## Phase 3: User Story 1 - Comprehensive Database Media Summary Report (Priority: P1) 🎯 MVP

**Goal**: Deliver the baseline reporting command (`cargo run -- report` / `status` / `audit`) that queries MongoDB and renders total file count, extension breakdown table, and status tallies (copied, unreplicated, duplicates, marked-for-delete, missing-exif, problems) in terminal and JSON formats.

**Independent Test**: Connect to a MongoDB collection populated with sample media documents. Run `cargo run -- report` and verify terminal output displays accurate counts and percentages. Run `cargo run -- report --format json` and verify valid JSON conforming to `report-json-schema.json`.

- [x] T005 [P] [US1] Create unit tests for audit report generation and calculation logic in `tests/audit_report_tests.rs`
- [x] T006 [US1] Implement `AuditService` in `src/application/audit_service.rs` to coordinate `StorageSink` report queries and timing telemetry
- [x] T007 [US1] Add `Report(ReportArgs)` subcommand with aliases `status` and `audit` to `enum Commands` in `src/main.rs`
- [x] T008 [US1] Implement terminal text and JSON output formatting for `CatalogAuditReport` in `src/main.rs`

**Checkpoint**: User Story 1 complete. Baseline CLI report is independently functional.

---

## Phase 4: User Story 2 - Targeted Manual Inspection with Top Sample Filepaths (Priority: P2)

**Goal**: Provide an inspection subcommand (`report inspect <category>` or `--inspect <category> --limit 3`) listing top sample filepaths and diagnostic context for manual operator verification.

**Independent Test**: Run `cargo run -- report inspect problems` and verify up to 3 filepaths with error reasons are displayed. Run `cargo run -- report inspect missing-exif` and verify up to 3 filepaths without EXIF dates are displayed.

- [x] T009 [P] [US2] Create unit and integration tests for top sample path querying and limits in `tests/audit_inspection_tests.rs`
- [x] T010 [US2] Implement sample inspection retrieval logic in `src/application/audit_service.rs` supporting `InspectionCategory` and custom limits
- [x] T011 [US2] Add `Inspect` subcommand and `--inspect` / `--limit` flags to `ReportArgs` in `src/main.rs` and render sample inspection results

**Checkpoint**: User Stories 1 and 2 functional. Operators can inspect top sample filepaths.

---

## Phase 5: User Story 3 - Contextual Next-Action Guidance (Priority: P3)

**Goal**: Analyze catalog health and append intelligent, prioritized next CLI command recommendations at the conclusion of the audit report.

**Independent Test**: Run `cargo run -- report` with different catalog states (unmastered items, pending replicas, unpruned duplicates, or all in sync) and verify the exact recommended CLI command string and rationale are displayed.

- [x] T012 [P] [US3] Create unit tests for the command recommendation decision tree in `tests/audit_recommendation_tests.rs`
- [x] T013 [US3] Implement priority-ordered command recommendation heuristic in `src/application/audit_service.rs`
- [x] T014 [US3] Integrate recommended action into terminal summary and JSON output schema in `src/main.rs`

**Checkpoint**: User Stories 1, 2, and 3 fully functional and integrated.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Quality gates, credential masking verification, and end-to-end quickstart validation

- [x] T015 [P] Verify credential masking and tracing telemetry during database audit queries in `src/application/audit_service.rs`
- [x] T016 Run quickstart validation scenarios in `specs/020-database-content-report/quickstart.md` and verify `cargo test` and `cargo clippy --all-targets -- -D warnings` pass cleanly

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 — BLOCKS all user stories
- **User Story 1 (Phase 3)**: Depends on Phase 2 — MVP delivery
- **User Story 2 (Phase 4)**: Depends on Phase 3 (extends Report CLI and AuditService)
- **User Story 3 (Phase 5)**: Depends on Phase 3 (extends Report presentation and AuditService)
- **Polish (Phase 6)**: Depends on Phases 3, 4, and 5 completion

### Parallel Opportunities

- T002 and T003 can run in parallel (different files in domain)
- T005, T009, T012 test tasks can be written in parallel
- T015 can run in parallel with documentation checks

---

## Implementation Strategy

### MVP First (User Story 1 Only)
1. Complete Phase 1 (T001)
2. Complete Phase 2 (T002 - T004)
3. Complete Phase 3 (T005 - T008)
4. Validate `cargo run -- report` displays total files, extension breakdown, and status counts.

### Incremental Delivery
1. Add US2 (T009 - T011) → Test `report inspect <category>`
2. Add US3 (T012 - T014) → Test next command suggestions
3. Polish (T015 - T016) → Run clippy, tests, and quickstart scenarios
