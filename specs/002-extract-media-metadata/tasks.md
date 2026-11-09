# Implementation Tasks: Media Metadata Extraction Engine

**Feature**: `002-extract-media-metadata`  
**Spec**: [spec.md](spec.md) | **Plan**: [plan.md](plan.md)  
**Status**: Ready for Implementation  

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Project dependency configuration and test fixture setup

- [X] T001 Add `nom-exif = "3.8"`, `kamadak-exif = "0.6"`, and `chrono = { version = "0.4", features = ["serde"] }` dependencies to `Cargo.toml`
- [X] T002 [P] Configure synthetic media test fixtures and corrupt sample files in `tests/fixtures/corrupt_samples/corrupt_exif.jpg` and `tests/fixtures/media_samples/sample.txt`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core domain entities, value objects, error types, and trait ports that MUST be complete before ANY user story can be implemented

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T003 Create metadata error enum `ExtractionError` (`FileNotFound`, `Io`, `CorruptHeader`, `UnsupportedFormat`) in `src/domain/errors.rs`
- [X] T004 [P] Implement domain entities and value objects in `src/domain/metadata_models.rs`:
  - `MediaMetadataDocument` with fields `id: Uuid`, `file_context: FileContext`, `temporal: TemporalMetadata`, `camera: Option<CameraMetadata>`, `exposure: Option<ExposureMetadata>`, `location: Option<GeoLocation>`, `dimensions: Option<ImageDimensions>`, `video: Option<VideoMetadata>`, `extraction_status: ExtractionStatus`
  - `FileContext` with constraints: `scanned_root: PathBuf` (non-empty path), `file_path: PathBuf` (absolute path), `relative_path: PathBuf` (non-empty relative path), `file_name: String` (non-empty string), `file_size_bytes: u64` ($\ge 0$), `extension: String` (normalized lowercase), `media_category: MediaCategory` (`Photo`, `Video`, `RawPhoto`)
  - `TemporalMetadata` with constraints: `timestamp_utc: Option<DateTime<Utc>>`, `iso_timestamp: String` (ISO 8601 UTC), `year: i32`, `month: u32` (`1..=12`), `day: u32` (`1..=31`), `hour: u32` (`0..=23`), `minute: u32` (`0..=59`), `second: u32` (`0..=59`), `source: TimestampSource` (`ExifDateTimeOriginal`, `ExifCreateDate`, `QuickTimeCreationTime`, `FileSystemModified`)
  - `CameraMetadata` with `make: Option<String>`, `model: Option<String>`, `lens_model: Option<String>`, `software: Option<String>`
  - `ExposureMetadata` with `focal_length_mm: Option<f64>`, `focal_length_35mm: Option<f64>`, `f_number: Option<f64>`, `exposure_time_seconds: Option<f64>`, `exposure_time_str: Option<String>`, `iso: Option<u32>`, `flash_fired: Option<bool>`
  - `GeoLocation` with constraints: `latitude: f64` (`[-90.0, 90.0]`), `longitude: f64` (`[-180.0, 180.0]`), `altitude_meters: Option<f64>`
  - `ImageDimensions` with `width: u32`, `height: u32`, `orientation: Option<u16>`
  - `VideoMetadata` with `duration_ms: u64`, `video_codec: Option<String>`, `audio_codec: Option<String>`, `frame_rate_fps: Option<f64>`, `bitrate_kbps: Option<u64>`, `rotation_degrees: Option<i32>`
  - `ExtractionStatus` with constraints: `status: ExtractionResultKind` (`Success`, `Partial`, `Failed`), `warnings: Vec<String>`, `error_message: Option<String>`
  - Derive `Debug, Clone, Serialize, Deserialize` for all models for MongoDB JSON compatibility
- [X] T005 [P] Define extraction ports `MediaMetadataExtractor` and `BatchMetadataExtractor` in `src/domain/ports.rs`
- [X] T006 Register domain models, errors, and ports in `src/domain/mod.rs` and re-export in `src/lib.rs`

**Checkpoint**: Core domain models, errors, and traits established — user story implementation can begin.

---

## Phase 3: User Story 1 - Comprehensive Photo & RAW Metadata Extraction (Priority: P1) 🎯 MVP

**Goal**: Parse embedded EXIF, TIFF, and XMP metadata from compressed photos (JPEG, PNG, HEIC, WebP) and camera RAW files (Canon CR2/CR3, Nikon NEF, Sony ARW, Adobe DNG, Fujifilm RAF, Olympus ORF) without decompressing full image payloads, extracting capture datetime, calendar facets (`year`, `month`, `day`), camera details, exposure settings, GPS coordinates, and dimensions.

**Independent Test**: Point the extractor at sample iPhone HEIC, Android JPEG, and camera RAW files; verify capture timestamps, calendar facets (`year`, `month`, `day`), camera body/lens, exposure parameters, and dimensions are accurately extracted into the standardized schema, with filesystem fallback for stripped photos.

### Tests for User Story 1

- [X] T007 [P] [US1] Create unit tests for temporal parsing and calendar faceting (`year`, `month`, `day`, `hour`, `minute`, `second`, and `FileSystemModified` fallback) in `tests/unit/date_facets_tests.rs`
- [X] T008 [P] [US1] Create unit tests for photo and camera RAW metadata extraction (EXIF tags, GPS coordinate normalization, camera make/model, exposure settings, dimensions) in `tests/unit/exif_parser_tests.rs`

### Implementation for User Story 1

- [X] T009 [US1] Implement temporal normalization helper functions in `src/domain/metadata_models.rs` converting EXIF date strings (`YYYY:MM:DD HH:MM:SS`), ISO 8601 strings, and `SystemTime` to `TemporalMetadata` with integer facets (`year`, `month`, `day`, `hour`, `minute`, `second`)
- [X] T010 [US1] Implement `ExifMetadataExtractor` adapter in `src/infrastructure/exif_parser.rs` using `nom-exif` and `kamadak-exif` fallback for JPEG, PNG, HEIC, WebP, and RAW files (CR2, CR3, NEF, ARW, DNG, RAF, ORF), with zero-copy header inspection and GPS coordinate normalization to signed decimal degrees (`[-90.0, 90.0]`, `[-180.0, 180.0]`)
- [X] T011 [US1] Implement fault isolation and filesystem timestamp fallback in `src/infrastructure/exif_parser.rs` when EXIF dates are missing or corrupted, setting `source = TimestampSource::FileSystemModified` and recording diagnostic warnings without panic
- [X] T012 [US1] Register `exif_parser` module in `src/infrastructure/mod.rs` and register `tests/unit/date_facets_tests.rs` and `tests/unit/exif_parser_tests.rs` in `tests/unit.rs`

**Checkpoint**: User Story 1 functional — photo and RAW metadata extraction works independently.

---

## Phase 4: User Story 2 - Video Container Metadata Parsing (Priority: P2)

**Goal**: Inspect video container headers (MP4, MOV, MKV, WebM, 3GP) to extract recording timestamps, playback duration in milliseconds, video/audio codecs, display dimensions, frame rate (fps), bitrate, and container rotation angle.

**Independent Test**: Extract metadata from sample MP4 and QuickTime MOV video clips; verify duration in ms, display dimensions, video codec, frame rate, and container recording timestamps are correctly extracted, including bounded seeking for files with `moov` atom at the file tail.

### Tests for User Story 2

- [X] T013 [P] [US2] Create unit tests for video container metadata extraction (duration, codecs, dimensions, rotation tags, and recording timestamps) in `tests/unit/video_parser_tests.rs`

### Implementation for User Story 2

- [X] T014 [US2] Implement `VideoMetadataExtractor` adapter in `src/infrastructure/video_parser.rs` using `nom-exif` to inspect video container headers (MP4, MOV, MKV, WebM, 3GP) for duration in milliseconds, video codec, audio codec, frame rate, and rotation angle
- [X] T015 [US2] Implement bounded header and tail seeking in `src/infrastructure/video_parser.rs` reading $\le 256\text{ KB}$ for atom inspection (`ftyp`, `moov`) to process multi-gigabyte files in $< 50\text{ ms}$ without buffering intermediate video stream bytes
- [X] T016 [US2] Register `video_parser` module in `src/infrastructure/mod.rs` and register `tests/unit/video_parser_tests.rs` in `tests/unit.rs`

**Checkpoint**: User Stories 1 and 2 functional — both photo/RAW and video container extraction work independently.

---

## Phase 5: User Story 3 - Normalized JSON Export for Database Ingestion (Priority: P3)

**Goal**: Emit all extracted media information in a strictly validated, normalized JSON document structure containing folder context, file paths, calendar facets, technical specifications, and error diagnostics, suitable for direct bulk insertion into MongoDB via single-file CLI and streaming pipeline modes.

**Independent Test**: Run extraction on a mixed batch of photos, videos, RAW files, and corrupted headers; verify that all emitted records conform to the JSON schema, validate that required date facets (`year`, `month`, `day`) are present as integers, and ensure corrupted files output valid diagnostic records rather than terminating the process.

### Tests for User Story 3

- [X] T017 [P] [US3] Create integration tests for pipeline streaming, schema compliance, and diagnostic reporting on corrupted files in `tests/integration/metadata_pipeline_tests.rs`

### Implementation for User Story 3

- [X] T018 [US3] Implement `MetadataExtractionEngine` in `src/application/metadata_engine.rs` implementing `MediaMetadataExtractor` and `BatchMetadataExtractor` traits, routing files to `ExifMetadataExtractor` or `VideoMetadataExtractor` based on `MediaCategory`, offloading decoding to `rayon` thread pools, and maintaining bounded memory (< 100 MB)
- [X] T019 [US3] Register `metadata_engine` module in `src/application/mod.rs` and re-export in `src/lib.rs`
- [X] T020 [US3] Implement `extract` CLI subcommand in `src/main.rs` to extract and output normalized JSON for a single media file path
- [X] T021 [US3] Implement `extract-stream` CLI subcommand in `src/main.rs` to consume scanned media batches or NDJSON from stdin, execute concurrent extraction across Rayon worker threads, and emit streaming NDJSON records to stdout
- [X] T022 [US3] Register `tests/integration/metadata_pipeline_tests.rs` in `tests/integration.rs`

**Checkpoint**: All user stories complete — end-to-end pipeline ready for single-file and batch NDJSON extraction.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Performance benchmarks, documentation, linting, and quality gate verification

- [X] T023 [P] Implement extraction performance benchmark in `benches/metadata_bench.rs` measuring extraction throughput against the $\ge 250$ files/sec per core target
- [X] T024 [P] Update `README.md` with usage documentation, CLI flags, JSON schemas, and streaming pipeline examples for `extract` and `extract-stream`
- [X] T025 Run automated quality gates: verify clean compilation with `cargo check`, test execution with `cargo test`, code formatting with `cargo fmt --check`, and strict lints with `cargo clippy --all-targets -- -D warnings`
- [X] T026 Execute end-to-end validation scenarios per `specs/002-extract-media-metadata/quickstart.md` (single file photo extraction, video container extraction, scan-to-extract streaming pipeline)

---

## Dependencies & Execution Order

### Phase Dependencies

```
┌─────────────────────────────────┐
│   Phase 1: Setup (T001-T002)    │
└────────────────┬────────────────┘
                 │
                 ▼
┌─────────────────────────────────┐
│ Phase 2: Foundational (T003-T006)│
└───────┬─────────────────┬───────┘
        │                 │
        ▼                 ▼
┌──────────────┐   ┌──────────────┐
│Phase 3: US1  │   │Phase 4: US2  │
│Photo/RAW EXIF│   │Video Headers │
│ (T007-T012)  │   │ (T013-T016)  │
└───────┬──────┘   └──────┬───────┘
        │                 │
        └────────┬────────┘
                 │
                 ▼
┌─────────────────────────────────┐
│Phase 5: US3 Normalized Pipeline │
│ (T017-T022)                     │
└────────────────┬────────────────┘
                 │
                 ▼
┌─────────────────────────────────┐
│   Phase 6: Polish (T023-T026)   │
└─────────────────────────────────┘
```

- **Setup (Phase 1)**: No dependencies — can start immediately.
- **Foundational (Phase 2)**: Depends on Phase 1 completion — BLOCKS all user stories.
- **User Stories (Phases 3 & 4)**: Depend on Phase 2 completion. Can be implemented in parallel or sequentially (US1 MVP first, then US2).
- **User Story 3 (Phase 5)**: Integrates US1 and US2 into the unified pipeline engine and CLI subcommands.
- **Polish (Phase 6)**: Depends on all user stories being implemented.

### Parallel Opportunities

- **Phase 1**: T002 can run in parallel with T001.
- **Phase 2**: T004 and T005 can run in parallel once T003 is defined.
- **Phase 3 (US1)**: T007 and T008 can be written concurrently.
- **Phase 4 (US2)**: T013 can be written independently while T010/T011 are being implemented.
- **Phase 6**: T023 and T024 can be implemented in parallel.

---

## Parallel Example: User Story 1

```bash
# Launch test creation for User Story 1 in parallel:
Task: "Create unit tests for temporal parsing and calendar faceting in tests/unit/date_facets_tests.rs"
Task: "Create unit tests for photo and camera RAW metadata extraction in tests/unit/exif_parser_tests.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (`Cargo.toml`, test sample fixtures)
2. Complete Phase 2: Foundational (Core models in `src/domain/metadata_models.rs`, errors, ports)
3. Complete Phase 3: User Story 1 (`ExifMetadataExtractor`, calendar faceting)
4. **STOP and VALIDATE**: Verify photo and RAW metadata extraction with `cargo test --test unit date_facets_tests exif_parser_tests`

### Incremental Delivery

1. Setup + Foundational → Domain types ready
2. Add US1 → Photo & RAW metadata extraction working (MVP!)
3. Add US2 → Video container metadata extraction working
4. Add US3 → Full streaming engine & CLI commands (`extract`, `extract-stream`)
5. Add Polish → Benchmarks, README updates, formatting, and clippy clean checks


---

## Phase 7: Convergence

- [X] T027 [CRITICAL] Instrument metadata extractors and engine with tracing spans and telemetry metrics per Constitution III (partial)
- [X] T028 Implement video container transform matrix rotation parsing, display dimension adjustment, and track codec extraction in src/infrastructure/video_parser.rs per FR-010, US2/AC1, US2/AC2 (partial)
- [X] T029 Extend kamadak-exif fallback in src/infrastructure/exif_parser.rs to extract camera, exposure, and dimension metadata for RAW/TIFF files per FR-003, FR-007, FR-008 (partial)
- [X] T030 Add unit test coverage for successful metadata extraction on valid EXIF and video container headers in tests/unit/exif_parser_tests.rs and tests/unit/video_parser_tests.rs per T008, T013, US1/AC1, US2/AC1 (partial)
- [X] T031 Expand benches/metadata_bench.rs to measure extraction throughput against the >= 250 files/sec per core target per T023, SC-004 (partial)
