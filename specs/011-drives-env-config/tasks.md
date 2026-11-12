# Tasks: Sample Drive Configuration and Environment-Driven Drive Declarations

**Input**: Design documents from `/specs/011-drives-env-config/`
**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md, data-model.md, contracts/

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: Which user story this task belongs to (`[US1]`, `[US2]`, `[US3]`)
- Exact file paths included in every task

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Establish sample file layout and baseline test scaffolding

- [X] T001 [P] Create dummy drive template `drives.example.yaml` at repository root showcasing `primary`, `replica1`, `replica2`, and `staging` dummy groups
- [X] T002 [P] Add environment variable drive documentation to `.env.example`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core string interpolation and variable resolver utilities

**⚠️ CRITICAL**: Foundational tasks must complete before user story implementation

- [X] T003 Implement `interpolate_env_vars` function supporting `${VAR}` and `${VAR:-default}` syntax in `src/infrastructure/drives_config.rs`
- [X] T004 Add unit tests for `interpolate_env_vars` in `tests/unit/drives_config_tests.rs`

**Checkpoint**: Variable interpolation functional; user stories unblocked.

---

## Phase 3: User Story 1 - Ready-to-Use Sample Drive Configuration Template (Priority: P1) 🎯 MVP

**Goal**: Ensure `drives.example.yaml` is fully valid, can be copied to `drives.yaml`, and loads cleanly via `DrivesConfigLoader`.

**Independent Test**: Load and validate `drives.example.yaml` with dummy paths through `DrivesConfigLoader::load_from_path` in an automated test.

- [X] T005 [P] [US1] Unit test in `tests/unit/drives_config_tests.rs` asserting `drives.example.yaml` parses and validates cleanly
- [X] T006 [US1] Validate group hierarchy and canonical master directories in `drives.example.yaml`

**Checkpoint**: User Story 1 verified; sample template can be copied and used immediately.

---

## Phase 4: User Story 2 - Environment Variable Configuration via `.env` (Priority: P1)

**Goal**: Dynamically assemble a valid `DriveConfiguration` directly from `DRIVE_PRIMARY_*` and `DRIVE_REPLICA<N>_*` environment variables without requiring a YAML file.

**Independent Test**: Unset config path, define `DRIVE_PRIMARY_MOUNT` and `DRIVE_REPLICA1_MOUNT` in test process, invoke `synthesize_from_env()`, and assert primary (100) and replica1 (80) drive definitions exist.

- [X] T007 [US2] Implement `synthesize_from_env()` scanning `DRIVE_PRIMARY_*` and `DRIVE_REPLICA<N>_*` in `src/infrastructure/drives_config.rs`
- [X] T008 [US2] Implement master directory default fallback (`<mount>/SPSA/media/master` or `<mount>/media/master`) in `src/infrastructure/drives_config.rs`
- [X] T009 [P] [US2] Unit tests verifying `synthesize_from_env()` with primary and multiple replicas in `tests/unit/drives_config_tests.rs`
- [X] T010 [P] [US2] Unit test asserting missing `DRIVE_PRIMARY_MOUNT` returns descriptive `DeduplicationError::ConfigError` in `tests/unit/drives_config_tests.rs`

**Checkpoint**: User Story 2 verified; pure `.env` configuration works out of the box.

---

## Phase 5: User Story 3 - Environment Variable Expansion Inside `drives.yaml` (Priority: P2)

**Goal**: Automatically apply `interpolate_env_vars` when loading YAML/TOML files from disk in `load_from_path` or `load_from_str`.

**Independent Test**: Load a YAML string containing `${DRIVE_PRIMARY_MOUNT:-/Volumes/Default}`, override via environment, and assert interpolated value is present.

- [X] T011 [US3] Integrate `interpolate_env_vars` inside `load_from_path` and `load_from_str` in `src/infrastructure/drives_config.rs`
- [X] T012 [P] [US3] Unit tests verifying `${VAR}` and `${VAR:-default}` interpolation in YAML strings in `tests/unit/drives_config_tests.rs`

**Checkpoint**: User Story 3 verified; `${VAR}` placeholders expand dynamically from `.env`.

---

## Phase 6: Polish & CLI Integration

**Purpose**: Wire `load_from_env_or_path` into CLI commands, update documentation, and execute quality gates

- [X] T013 Implement `load_from_env_or_path(Option<&Path>)` with fallback order (explicit path -> default `drives.yaml` if exists -> `.env` synthesis) in `src/infrastructure/drives_config.rs`
- [X] T014 Update `main.rs` `dedup` and `ingest` command handlers to use `load_from_env_or_path`
- [X] T015 [P] Update `README.md` and `apis.md` to document drive configuration via `.env` variables
- [X] T016 Run formatting and linter checks with `cargo fmt --check` and `cargo clippy --all-targets -- -D warnings`
- [X] T017 Run complete test suite with `cargo test`
- [X] T018 Fix short-flag collision on `DedupArgs` so `-c` routes to `--config` and `--channel-capacity` is long-only in `src/main.rs`
- [X] T019 Interpolate `<MONGODB_PASSWORD>` consistently in `resolve_db_config` across `dedup`, `prune`, and `ingest` subcommands in `src/main.rs`
- [X] T020 Fix BSON `Int64` sign wrapping for perceptual hashes with `deserialize_u64_or_signed` in `src/domain/dedup_models.rs` and `bson_to_json` in `src/infrastructure/mongodb_sink.rs`
- [X] T021 Add regression unit tests for negative `Int64` BSON deserialization in `tests/unit/phash_tests.rs` and `tests/unit/mongodb_config_tests.rs`

---

## Dependencies & Execution Order

- **Phase 1 (Setup)** $\rightarrow$ **Phase 2 (Foundational)**: Blocks all stories.
- **Phase 2** $\rightarrow$ **Phase 3 (US1: Sample Template)**.
- **Phase 2** $\rightarrow$ **Phase 4 (US2: Direct .env Synthesis)**.
- **Phase 2** $\rightarrow$ **Phase 5 (US3: YAML Var Expansion)**.
- **Phase 3, 4, 5** $\rightarrow$ **Phase 6 (Polish & CLI Integration)**.
