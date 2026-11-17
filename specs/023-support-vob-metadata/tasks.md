# Tasks: Support for VOB Video Extensions and DVD Metadata Files

**Input**: Design documents from `/specs/023-support-vob-metadata/`
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/vob-dvd-contract.md

## Format: `- [ ] [TaskID] [P?] [Story?] Description with file path`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: User story label (`[US1]`, `[US2]`, `[US3]`)

---

## Phase 1: Setup

**Purpose**: Baseline preparation and test harness verification

- [X] T001 Verify baseline test harness and classifier tests in `tests/unit/classifier_tests.rs`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core extension mapping and codec fallback

- [X] T002 Add `"vob" => MediaCategory::Video`, `"ifo" => MediaCategory::Sidecar`, `"bup" => MediaCategory::Sidecar` to `EXTENSION_MAP` in `src/infrastructure/classifier.rs`
- [X] T003 Add codec fallback for `"vob"` (`mpeg2`, `ac3/lpcm/dts`) in `src/infrastructure/video_parser.rs`

---

## Phase 3: User Story 1 - Media Discovery & Classification for VOB and DVD Metadata (Priority: P1) 🎯 MVP

**Goal**: Discover and classify `.vob`, `.ifo`, and `.bup` files during media scanning.

**Independent Test**: Run scanner on a directory containing `VTS_01_1.VOB`, `VTS_01_0.IFO`, and `VTS_01_0.BUP`; verify all 3 are cataloged in MongoDB with correct categories.

- [X] T004 [US1] Add unit tests in `tests/unit/classifier_tests.rs` verifying `.vob` (Video) and `.ifo`/`.bup` (Sidecar) classification with various casing (`VOB`, `vob`, `IFO`, `BUP`)
- [X] T005 [US1] Add video parser unit test in `tests/unit/video_parser_tests.rs` verifying `.vob` extraction and fallback metadata

---

## Phase 4: User Story 2 - Ingestion & Co-Location of VOB Videos and Companion DVD Metadata (Priority: P1)

**Goal**: Ingest `VTS_01_1.VOB` and co-locate companion `.ifo` and `.bup` files into the `Videos/` canonical master folder.

**Independent Test**: Ingest a folder containing `VTS_01_1.VOB`, `VTS_01_0.IFO`, and `VTS_01_0.BUP`; verify all 3 reside in `<Year>/<DD-Mon-YYYY>/Videos/`.

- [X] T006 [US2] Implement `is_companion_sidecar_stem_match` supporting DVD Title Sets (`VTS_xx_y` matching `VTS_xx_0` / `VTS_xx_y`, `VIDEO_TS` matching `VIDEO_TS`, and exact stems) in `src/application/transfer_service.rs`
- [X] T007 [US2] Implement `find_companion_sidecars` returning `Vec<PathBuf>` for multi-sidecar lookup (.ifo, .bup, .aae) in `src/application/transfer_service.rs`
- [X] T008 [US2] Implement `transfer_companion_sidecars_if_present` transferring all companion sidecars in `src/application/transfer_service.rs`
- [X] T009 [US2] Update `IngestService` sidecar companion resolution in `src/application/ingest_service.rs` to route DVD sidecars (`.ifo`, `.bup`) to `Videos/` alongside their companion video or as default folder
- [X] T010 [US2] Add integration tests in `tests/test_vob_dvd_metadata.rs` verifying discovery, ingestion, and co-location of `VTS_01_1.VOB`, `VTS_01_0.IFO`, and `VTS_01_0.BUP` into `Videos/`

---

## Phase 5: User Story 3 - Replica Synchronization & Pruning of VOB Media and Metadata (Priority: P2)

**Goal**: Synchronize VOB and companion IFO/BUP files to replica drives and safely prune them from source drives.

**Independent Test**: Run replica sync and source prune on a folder with `VTS_01_1.VOB` and `.ifo`/`.bup`; verify replica matches master and source files/directories are cleaned.

- [X] T011 [US3] Update `DrivePruningService` companion sidecar verification to verify and prune all companion sidecars (`.ifo`, `.bup`) in `src/application/drive_pruner.rs`
- [X] T012 [US3] Add integration test in `tests/test_vob_dvd_metadata.rs` verifying replication of VOB and companion IFO/BUP files and pruning of source files

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Validation, documentation, and test suite execution

- [X] T013 Update `README.md` and `apis.md` to list VOB, IFO, and BUP in supported formats and sidecars
- [X] T014 Run full test suite `cargo test` and verify `cargo fmt --check` and `cargo clippy`
