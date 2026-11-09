# Tasks: Direct MongoDB Storage Sink

**Feature**: `005-mongodb-storage-sink`  
**Input**: Design documents from `specs/005-mongodb-storage-sink/` (`spec.md`, `plan.md`, `research.md`, `data-model.md`, `contracts/`, `quickstart.md`)  
**Prerequisites**: `spec.md`, `plan.md`, `data-model.md`, `contracts/mongodb-cli-contract.md`  

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Dependency configuration and shared storage types

- [X] T001 Add `mongodb = { version = "3.2", default-features = false, features = ["tokio-runtime", "compat-3-0-0"] }` to `Cargo.toml`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core domain abstractions, error models, and configuration types required by all user stories

**⚠️ CRITICAL**: No user story implementation can begin until this phase is complete

- [X] T002 Add `StorageError` strongly-typed error enum to `src/domain/errors.rs`
- [X] T003 Define `MetadataStorageSink` trait port and `BatchWriteStats` in `src/domain/ports.rs`
- [X] T004 Define `DatabaseSinkConfig` in `src/domain/metadata_models.rs` with defaults (`collection: "media_assets"`, `batch_size: 500`) and `sanitized_uri()` password masking

**Checkpoint**: Storage trait and configuration model ready — user story implementation can begin.

---

## Phase 3: User Story 1 - Direct Media Metadata Persistence to MongoDB (Priority: P1) 🎯 MVP

**Goal**: Enable users to specify `--mongodb-uri <URI>` (and optional `--collection`) to stream extracted metadata documents directly into MongoDB during scans.

**Independent Test**: Run the scanner with `--mongodb-uri` on test fixtures; verify records are persisted in the target MongoDB collection with complete metadata, calendar facets, and content fingerprints.

### Tests for User Story 1

- [X] T005 [P] [US1] Create unit tests in `tests/unit/mongodb_config_tests.rs` verifying `DatabaseSinkConfig` URI parsing, default collection assignment, and credential masking
- [X] T006 [P] [US1] Create integration tests in `tests/integration/mongodb_sink_tests.rs` verifying mock or live storage sink batch persistence

### Implementation for User Story 1

- [X] T007 [US1] Implement `MongoDbStorageSink` adapter in `src/infrastructure/mongodb_sink.rs` implementing `MetadataStorageSink` with client connection and bulk write execution
- [X] T008 [US1] Export `mongodb_sink` module in `src/infrastructure/mod.rs` and `src/lib.rs`
- [X] T009 [US1] Add `--mongodb-uri` and `--collection` CLI arguments to `Cli` and `ExtractArgs` in `src/main.rs`
- [X] T010 [US1] Integrate `MongoDbStorageSink` into `run_extract` in `src/main.rs` to stream extracted document batches directly to MongoDB

**Checkpoint**: User Story 1 functional — users can stream scan results directly into MongoDB.

---

## Phase 4: User Story 2 - Idempotent Upsert & Re-Scan Synchronization (Priority: P2)

**Goal**: Ensure re-scanning directories updates existing documents rather than creating duplicates, and automatically initialize required query indexes.

**Independent Test**: Scan a folder into MongoDB; record document count. Re-run scan on the same folder; verify document count is unchanged and 0 duplicate documents exist.

### Tests for User Story 2

- [X] T011 [P] [US2] Create integration test in `tests/integration/mongodb_sink_tests.rs` verifying `ensure_indexes` registers unique index on `file_context.file_path` and compound indexes on temporal and fingerprint fields
- [X] T012 [P] [US2] Create integration test in `tests/integration/mongodb_sink_tests.rs` verifying idempotent upsert behavior (0 duplicate records upon repeated batch writes)

### Implementation for User Story 2

- [X] T013 [US2] Implement `ensure_indexes` in `src/infrastructure/mongodb_sink.rs` creating unique index on `file_context.file_path`, compound index on `temporal.year/month`, and indexes on `fingerprint.cryptographic_hash` and `fingerprint.phash`
- [X] T014 [US2] Implement unordered `bulk_write` with `UpdateOneModel` and `upsert: true` matching on `file_context.file_path` in `src/infrastructure/mongodb_sink.rs`

**Checkpoint**: User Stories 1 and 2 functional — database synchronization is completely idempotent with automated indexing.

---

## Phase 5: User Story 3 - Connection Resilience, Authentication & Ingestion Telemetry (Priority: P3)

**Goal**: Validate connection/credentials upfront before directory walking, handle transient write blips with retries, and display live write throughput on stderr.

**Independent Test**: Pass an invalid connection URI; verify scan fails fast in $< 2$ seconds before starting filesystem traversal, masking passwords in error messages.

### Tests for User Story 3

- [X] T015 [P] [US3] Create unit tests in `tests/unit/mongodb_config_tests.rs` verifying invalid URIs and unreachable hosts fail fast with descriptive sanitized errors

### Implementation for User Story 3

- [X] T016 [US3] Implement fail-fast `ping()` connection and authentication check in `MongoDbStorageSink` and invoke it in `run_extract` in `src/main.rs` prior to starting directory traversal
- [X] T017 [US3] Add exponential backoff retry mechanism for transient network or write errors in `MongoDbStorageSink::save_batch` in `src/infrastructure/mongodb_sink.rs`
- [X] T018 [US3] Extend live scan telemetry heartbeat on `stderr` in `src/main.rs` to display database write counts, throughput rate, and write error tallies

**Checkpoint**: All user stories functional — robust connection validation, retries, and live feedback in place.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Test registration, documentation, and quality gates

- [X] T019 Register `mongodb_sink_tests.rs` in `tests/integration.rs` and `mongodb_config_tests.rs` in `tests/unit.rs`
- [X] T020 Update `README.md` documenting `--mongodb-uri` and `--collection` options with connection string examples, mongosh queries, and sanitization notes
- [X] T021 Run automated quality gates: verify clean compilation with `cargo check`, test execution with `cargo test`, code formatting with `cargo fmt --check`, and strict lints with `cargo clippy --all-targets -- -D warnings`
- [X] T022 Validate quickstart scenarios per `specs/005-mongodb-storage-sink/quickstart.md`

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
│Phase 2: Foundational (T002-T004)│
└────────────────┬────────────────┘
                 │
                 ▼
┌─────────────────────────────────┐
│Phase 3: US1 Direct Mongo Ingest │
│ (T005-T010)                     │
└────────────────┬────────────────┘
                 │
                 ▼
┌─────────────────────────────────┐
│Phase 4: US2 Idempotent Upsert   │
│ (T011-T014)                     │
└────────────────┬────────────────┘
                 │
                 ▼
┌─────────────────────────────────┐
│Phase 5: US3 Telemetry & Retries │
│ (T015-T018)                     │
└────────────────┬────────────────┘
                 │
                 ▼
┌─────────────────────────────────┐
│   Phase 6: Polish (T019-T022)   │
└─────────────────────────────────┘
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)
1. Complete Phase 1: Setup (`T001`).
2. Complete Phase 2: Foundational (`T002`–`T004`).
3. Complete Phase 3: User Story 1 (`T005`–`T010`).
4. **STOP and VALIDATE**: Verify `metadata-extractor <PATH> --mongodb-uri ...` persists media assets into MongoDB.

### Incremental Delivery
1. Foundation & Trait Ports (`T001`–`T004`)
2. MVP Ingestion (`T005`–`T010`)
3. Idempotent Upsert & Automated Indexing (`T011`–`T014`)
4. Resilient Retries & Telemetry (`T015`–`T018`)
5. Quality Gates & Documentation (`T019`–`T022`)
