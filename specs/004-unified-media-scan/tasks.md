# Tasks: Unified Media Metadata & Fingerprint Scanner

**Feature**: `004-unified-media-scan`  
**Input**: Design documents from `specs/004-unified-media-scan/` (`spec.md`, `plan.md`, `research.md`, `data-model.md`, `contracts/`, `quickstart.md`)  
**Prerequisites**: `spec.md`, `plan.md`, `data-model.md`, `contracts/unified-cli-contract.md`  

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Test fixture verification and harness setup for single-command testing

- [X] T001 Review and verify synthetic media test fixtures in `tests/fixtures/` for single-command recursive directory testing (valid EXIF, RAW preview, video container, corrupt samples)

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core CLI argument model and file sink abstractions required by all user stories

**⚠️ CRITICAL**: No user story implementation can begin until this phase is complete

- [X] T002 Add `-o, --output <FILE>` optional destination argument to extraction options in `src/main.rs`
- [X] T003 Refactor top-level `Cli` struct in `src/main.rs` to accept an optional positional argument `path: Option<PathBuf>` alongside optional subcommands `command: Option<Commands>`

**Checkpoint**: CLI argument model supports top-level path invocation — user story implementation can begin.

---

## Phase 3: User Story 1 - Single-Command Recursive Media Metadata Extraction (Priority: P1) 🎯 MVP

**Goal**: Enable users to run `metadata-extractor <PATH>` with 1 mandatory input path to recursively discover all media files across all subdirectories and emit complete metadata documents.

**Independent Test**: Execute `cargo run -- /path/to/media_directory` with no subcommands; verify all nested photos, videos, and RAW files are traversed and output as structured JSON documents to stdout.

### Tests for User Story 1

- [X] T004 [P] [US1] Create integration test in `tests/integration/unified_scan_tests.rs` verifying single-command recursive directory traversal and extraction on nested folders with only 1 mandatory path argument
- [X] T005 [P] [US1] Create integration test in `tests/integration/unified_scan_tests.rs` verifying single-file direct path extraction without subcommands

### Implementation for User Story 1

- [X] T006 [US1] Implement top-level execution dispatch in `main()` in `src/main.rs` routing `cli.path` directly to the unified extraction pipeline
- [X] T007 [US1] Register `tests/integration/unified_scan_tests.rs` in `tests/integration.rs`

**Checkpoint**: User Story 1 functional — users can run `metadata-extractor <PATH>` with 1 mandatory input.

---

## Phase 4: User Story 2 - Integrated Content Fingerprinting and Perceptual Hashes (Priority: P2)

**Goal**: Ensure every document emitted by the unified single-command scan contains the complete `fingerprint` section with BLAKE3 cryptographic hash, sample digest, 64-bit DCT pHash, rotated variants, and low-entropy detection.

**Independent Test**: Run `metadata-extractor <PATH>` on test fixtures containing photos, videos, downscaled copies, and camera RAW files; inspect output documents to verify `fingerprint` is populated on 100% of extractable assets.

### Tests for User Story 2

- [X] T008 [P] [US2] Create integration test in `tests/integration/unified_scan_tests.rs` verifying that 100% of emitted documents contain `fingerprint.cryptographic_hash`, `fingerprint.sample_digest`, and for photos: `fingerprint.phash`, `fingerprint.phash_hex`, `fingerprint.rotated_hashes`, and `fingerprint.is_low_entropy`

### Implementation for User Story 2

- [X] T009 [US2] Validate that camera RAW files and photos processed via unified single-command dispatch compute and embed complete perceptual hashes and rotation variants in `src/application/metadata_engine.rs`

**Checkpoint**: User Stories 1 and 2 functional — unified scan produces complete metadata and visual fingerprints.

---

## Phase 5: User Story 3 - Flexible Output Destinations and Live Feedback (Priority: P3)

**Goal**: Support saving directly to an output file (`-o, --output <FILE>`) and pretty printing (`-p, --pretty`), while streaming live progress on `stderr`.

**Independent Test**: Run `metadata-extractor <DIR> -o catalog.ndjson`; verify records are written directly to the file without terminal noise, and verify `stderr` displays non-intrusive progress.

### Tests for User Story 3

- [X] T010 [P] [US3] Create integration test in `tests/integration/unified_scan_tests.rs` verifying `-o / --output <FILE>` writes valid NDJSON records directly to disk

### Implementation for User Story 3

- [X] T011 [US3] Implement buffered asynchronous file output writer in `run_extract` in `src/main.rs` when `--output` is specified
- [X] T012 [US3] Ensure real-time telemetry heartbeats on `stderr` report visited folders, discovered assets, throughput fps, and skip counts without corrupting stdout or file output in `src/main.rs`

**Checkpoint**: All user stories complete — unified single command supports stdout streaming and direct file sinks.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Documentation, automated quality gates, and end-to-end validation

- [X] T013 Update `README.md` with the new unified single-command syntax (`metadata-extractor <PATH>`), output file options (`-o, --output`), and updated command reference table
- [X] T014 Run automated quality gates: verify clean compilation with `cargo check`, test execution with `cargo test`, code formatting with `cargo fmt --check`, and strict lints with `cargo clippy --all-targets -- -D warnings`
- [X] T015 Validate all quickstart scenarios in `specs/004-unified-media-scan/quickstart.md`

---

## Dependencies & Execution Order

### Phase Dependencies

```
┌─────────────────────────────────┐
│   Phase 1: Setup (T001)         │
└────────────────┬────────────────┘
                 │
                 ▼
┌─────────────────────────────────┐
│Phase 2: Foundational (T002-T003)│
└────────────────┬────────────────┘
                 │
                 ▼
┌─────────────────────────────────┐
│Phase 3: US1 Single Command MVP  │
│ (T004-T007)                     │
└────────────────┬────────────────┘
                 │
                 ▼
┌─────────────────────────────────┐
│Phase 4: US2 Hash Verification   │
│ (T008-T009)                     │
└────────────────┬────────────────┘
                 │
                 ▼
┌─────────────────────────────────┐
│Phase 5: US3 File Sink & Telemetry│
│ (T010-T012)                     │
└────────────────┬────────────────┘
                 │
                 ▼
┌─────────────────────────────────┐
│   Phase 6: Polish (T013-T015)   │
└─────────────────────────────────┘
```

### User Story Dependencies

- **User Story 1 (P1)**: Depends on Foundational Phase (T002, T003). Delivers core MVP.
- **User Story 2 (P2)**: Extends US1 output verification.
- **User Story 3 (P3)**: Adds file sink and output routing to US1/US2.

---

## Parallel Opportunities

- `T004` and `T005` can be authored together in `tests/integration/unified_scan_tests.rs`.
- `T008` and `T010` test cases can be authored in parallel with implementation.

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup (T001)
2. Complete Phase 2: Foundational (T002, T003)
3. Complete Phase 3: User Story 1 (T004 - T007)
4. **STOP and VALIDATE**: Verify `cargo run -- <PATH>` works with 1 mandatory input path.

### Incremental Delivery

1. Foundation ready (T002-T003)
2. MVP delivered: single command recursive extraction (T004-T007)
3. Content fingerprints and perceptual hashes verified (T008-T009)
4. Direct file output sink and clean telemetry added (T010-T012)
5. Documentation and automated quality gates verified (T013-T015)
