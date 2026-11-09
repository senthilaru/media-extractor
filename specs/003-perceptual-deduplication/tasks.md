# Implementation Tasks: Perceptual Fingerprinting & Hash Extraction

**Feature Branch**: `003-perceptual-deduplication`  
**Spec**: [spec.md](spec.md) | **Plan**: [plan.md](plan.md)  
**Status**: Completed  

---

## Phase 1: Setup & Data Model Alignment

- [X] T001 Update `MediaMetadataDocument` in `src/domain/metadata_models.rs` to include `pub fingerprint: Option<MediaFingerprint>`
- [X] T002 Update `MediaFingerprint` in `src/domain/dedup_models.rs` to add `pub phash_hex: Option<String>` and update constructor logic
- [X] T003 Update synthetic media test fixtures if needed in `tests/fixtures/dedup_samples/`

---

## Phase 2: Perceptual Hashing & Fingerprinting Integration into Metadata Engine

- [X] T004 Integrate `Blake3MediaHasher` into `MetadataExtractionEngine` in `src/application/metadata_engine.rs` to automatically compute cryptographic hashes (BLAKE3) and perceptual visual hashes (DCT pHash) for every extracted media file
- [X] T005 Populate `doc.fingerprint` in `MetadataExtractionEngine::extract_file` with the computed fingerprint, isolating any per-file decode errors into `doc.extraction_status.warnings`
- [X] T006 Ensure camera RAW embedded preview JPEGs are passed to the perceptual hasher so RAW files receive pHash in their metadata document

---

## Phase 3: Verification & CLI Pipeline Validation

- [X] T007 Update unit tests in `tests/unit/exif_parser_tests.rs` and `tests/unit/video_parser_tests.rs` to verify `doc.fingerprint` is present and populated with hashes
- [X] T008 Update integration tests in `tests/integration/metadata_pipeline_tests.rs` and `tests/integration/dedup_pipeline_tests.rs` verifying that metadata documents emitted by `extract` and `extract-stream` contain perceptual hashes without running duplicate scanning/marking passes
- [X] T009 Update `main.rs` and `README.md` to reflect that hashes are embedded directly in the output JSON of `extract` and `extract-stream`
- [X] T010 Run automated quality gates: `cargo check`, `cargo test`, `cargo fmt --check`, and `cargo clippy --all-targets -- -D warnings`
