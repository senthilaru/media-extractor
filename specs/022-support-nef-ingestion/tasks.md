# Tasks: Ingest Nikon NEF Raw Format

**Input**: Design documents from `specs/022-support-nef-ingestion/`  
**Prerequisites**: plan.md, spec.md, research.md, data-model.md, contracts/  

## Phase 1: Setup

- [X] T001 Inspect Nikon NEF sample files and verify test fixtures in `tests/fixtures/`

---

## Phase 2: Foundational (Blocking Prerequisites)

- [X] T002 [P] Implement TIFF SubIFD dimension reader in `src/infrastructure/exif_parser.rs` to extract full sensor resolution for RAW/NEF files
- [X] T003 [P] Enhance `filter_unmodified_items` in `src/infrastructure/mongodb_sink.rs` to re-extract documents that failed or lack cryptographic fingerprints

---

## Phase 3: User Story 1 - Discover and Ingest Nikon NEF Files (Priority: P1) 🎯 MVP

**Goal**: Ingest unmastered Nikon `.NEF` files from external source drives into primary master storage under `<Year>/<DD-Mon-YYYY>/PhotosRaw/<filename>`, registering them as verified canonical masters.

**Independent Test**: Connect a source directory with `.NEF` files, run `cargo run -- extract` and `cargo run -- organize -c drives.yaml --dry-run`, and verify that NEF files are recognized, extracted, and scheduled for ingestion into `PhotosRaw/`.

### Tests for User Story 1
- [X] T004 [P] [US1] Unit test for NEF metadata extraction verifying `RawPhoto` category, full sensor dimensions, and camera metadata in `tests/test_nef_extraction.rs`
- [X] T005 [P] [US1] Unit test for self-healing incremental filtering re-extracting failed records in `tests/test_incremental_filter.rs`

### Implementation for User Story 1
- [X] T006 [US1] Integrate TIFF SubIFD dimension fallback into `ExifMetadataExtractor::extract_file` in `src/infrastructure/exif_parser.rs`
- [X] T007 [US1] Update `filter_unmodified_items` in `src/infrastructure/mongodb_sink.rs` to exclude `Failed` or un-fingerprinted records from being skipped
- [X] T008 [US1] Verify that `IngestService` routes `RawPhoto` category NEF files to `PhotosRaw/` in `src/application/ingest_service.rs`

---

## Phase 4: User Story 2 - Accurate Full Sensor Metadata Extraction for NEF (Priority: P2)

**Goal**: Ensure Nikon NEF files record true sensor dimensions ($\ge 1000$ px), camera model, and exposure settings, and are protected from false deduplication against companion JPEG files.

**Independent Test**: Extract a `.NEF` file and verify dimensions $\ge 6000 \times 4000$. Compare against a companion `.JPG` and verify both exist in catalog without deduplication collision.

### Implementation for User Story 2
- [X] T009 [US2] Update `MediaQualityRank` computation in `src/application/dedup_engine.rs` to ensure full sensor dimensions and `RawPhoto` tier rank NEF files appropriately
- [X] T010 [US2] Ensure companion `.JPG` and `.NEF` files with identical base stem remain distinct assets in `src/application/dedup_engine.rs`

---

## Phase 5: User Story 3 - Replica Synchronization & Safe Source Pruning (Priority: P3)

**Goal**: Mirror ingested NEF files to backup replica drives and safely prune source files only after bitwise verification.

**Independent Test**: Run replica synchronization on ingested NEF files, verify exact byte matching on replica drives, and verify pruning checks.

### Implementation for User Story 3
- [X] T011 [US3] Verify replica synchronization mirrors `PhotosRaw/` NEF assets to backup drives in `src/application/replica_sync_service.rs`
- [X] T012 [US3] Verify source pruning safely deletes source NEF files only after bitwise verification on master and replica in `src/application/drive_pruner.rs`

---

## Phase 6: Polish & Cross-Cutting Concerns

- [X] T013 [P] Run full test suite with `cargo test` to ensure zero regressions
- [X] T014 Run validation on real Nikon NEF files on external drive and verify `cargo run -- report -c drives.yaml`
