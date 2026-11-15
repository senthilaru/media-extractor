# Tasks: Resolution-Aware Deduplication Quality Selection

**Input**: Design documents from `specs/021-prefer-highest-resolution/`

**Prerequisites**: `plan.md`, `spec.md`, `research.md`, `data-model.md`, `contracts/dedup-quality-ranking.md`

## Format: `- [ ] [TaskID] [P?] [Story?] Description with file path`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (e.g., US1, US2, US3)

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Verify project configuration and dependencies

- [X] T001 [P] Verify project dependencies and environment in Cargo.toml

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core data models and ranking logic required by all user stories

- [X] T002 [P] Implement MediaQualityRank struct and comparison ordering in src/domain/dedup_models.rs
- [X] T003 [P] Add unit test suite for MediaQualityRank ordering in tests/unit.rs

---

## Phase 3: User Story 1 - Prefer Highest Resolution in Duplicate Groups (Priority: P1) 🎯 MVP

**Goal**: Automatically select the highest resolution image as the keeper and mark lower-resolution copies for removal.

**Independent Test**: Evaluate a cluster with 4000x3000, 1920x1080, and 300x300 copies; verify 4000x3000 is keeper and others are marked `MarkForRemoval`.

- [X] T004 [US1] Implement helper to extract pixel dimensions and compute quality rank in src/application/dedup_engine.rs
- [X] T005 [US1] Update score_canonical_candidate to strictly prioritize pixel count without score capping in src/application/dedup_engine.rs
- [X] T006 [US1] Update evaluate_content_group to sort candidates by MediaQualityRank before drive priority in src/application/dedup_engine.rs
- [X] T007 [P] [US1] Add integration test for mixed-resolution clusters (4000x3000 vs 1920x1080 vs 300x300) in tests/test_dedup_quality.rs

---

## Phase 4: User Story 2 - File Size Tie-Breaking for Equal Resolution (Priority: P2)

**Goal**: Preserve larger file size when pixel dimensions match, marking heavily compressed smaller copies for removal.

**Independent Test**: Evaluate duplicate assets with identical 1920x1080 resolution but different file sizes; verify larger file is chosen as keeper.

- [X] T008 [US2] Update candidate ranking tie-breaker to prioritize larger file_size_bytes for identical pixel dimensions in src/application/dedup_engine.rs
- [X] T009 [P] [US2] Add integration test verifying larger file size wins over compressed smaller file for equal resolution in tests/test_dedup_quality.rs

---

## Phase 5: User Story 3 - Upgrading Lower-Resolution Assets in Master Archive (Priority: P3)

**Goal**: Identify when an unmastered candidate has higher resolution than an existing copy on the primary master drive, setting the superior copy to `NeedsIngestToMaster`.

**Independent Test**: Evaluate a group where primary master has a 1024x768 image and staging has a 4032x3024 copy; verify staging copy is marked `NeedsIngestToMaster`.

- [X] T010 [US3] Implement master upgrade evaluation in evaluate_content_group when non-master candidate has higher quality than existing primary master in src/application/dedup_engine.rs
- [X] T011 [P] [US3] Add integration test verifying master upgrade sets superior candidate to NeedsIngestToMaster in tests/test_dedup_quality.rs

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: System validation, code hygiene, and documentation

- [X] T012 [P] Run full test suite and clippy checks across all targets with cargo clippy --all-targets -- -D warnings
- [X] T013 [P] Update user documentation in README.md and apis.md regarding resolution-first deduplication policy

---

## Dependencies & Execution Order

- **Phase 1 (Setup)**: Can run immediately.
- **Phase 2 (Foundational)**: Depends on T001; BLOCKS Phase 3, 4, 5.
- **Phase 3 (US1)**: Depends on T002, T003. Delivers MVP.
- **Phase 4 (US2)**: Builds upon US1 scoring logic.
- **Phase 5 (US3)**: Builds upon US1/US2 in `evaluate_content_group`.
- **Phase 6 (Polish)**: Runs after all implementation tasks.
