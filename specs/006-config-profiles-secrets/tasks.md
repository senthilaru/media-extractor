# Tasks: Externalized Configuration, Profiles, and Secret Management

**Feature**: `006-config-profiles-secrets`
**Input**: Design documents from `specs/006-config-profiles-secrets/` (`spec.md`, `plan.md`, `research.md`, `data-model.md`, `contracts/`, `quickstart.md`)
**Prerequisites**: `spec.md`, `plan.md`, `data-model.md`, `contracts/cli-profile-contract.md`

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Dependency configuration and environment template

- [X] T001 Add `dotenvy = "0.15"` to `Cargo.toml`
- [X] T002 [P] Create `.env.example` in repository root with sanitized environment variable placeholders

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Core domain profile enums, configuration errors, and secret interpolation logic required by all user stories

**⚠️ CRITICAL**: No user story implementation can begin until this phase is complete

- [X] T003 Define `AppProfile` enum (`Local`, `Production`) with `clap::ValueEnum`, `serde`, and `Display` in `src/domain/metadata_models.rs`
- [X] T004 Add `ConfigurationError(String)` variant to `StorageError` in `src/domain/errors.rs`
- [X] T005 Implement `interpolate_secret(uri: &str, password: Option<&str>)` with URL encoding and placeholder replacement in `DatabaseSinkConfig` in `src/domain/metadata_models.rs`

**Checkpoint**: Foundation ready — user story implementation can begin.

---

## Phase 3: User Story 1 - Profile-Driven Operational Execution (Priority: P1) 🎯 MVP

**Goal**: Allow operators to select operational profiles (`local`, `production`) via CLI or environment variable, adjusting configuration defaults and validation rules.

**Independent Test**: Invoke application with `--profile local` and `--profile production`, verifying correct profile resolution and validation.

### Tests for User Story 1

- [X] T006 [P] [US1] Create unit tests in `tests/unit/profile_config_tests.rs` verifying `AppProfile` parsing from strings and case-insensitive conversion

### Implementation for User Story 1

- [X] T007 [US1] Add `--profile` CLI option to `Cli` and `ExtractArgs` in `src/main.rs` with `APP_PROFILE` / `MEDIA_PROFILE` environment variable fallback
- [X] T008 [US1] Implement profile validation in `src/main.rs` (local default, strict production validation)

**Checkpoint**: User Story 1 functional — application supports runtime profiles.

---

## Phase 4: User Story 2 - Secure Credential Externalization & Secret Injection (Priority: P2)

**Goal**: Externalize database passwords via environment variables, safely interpolate secrets into connection URIs, and enforce fail-fast validation in production mode.

**Independent Test**: Pass database URI with placeholder and provide secret via `MONGODB_PASSWORD`; verify successful resolution and masked logging.

### Tests for User Story 2

- [X] T009 [P] [US2] Create unit tests in `tests/unit/profile_config_tests.rs` verifying secret interpolation, URL encoding for special characters, and fail-fast validation on missing production secrets

### Implementation for User Story 2

- [X] T010 [US2] Integrate `dotenvy` loading (`.env.{profile}` then `.env`) at application entry point in `src/main.rs`
- [X] T011 [US2] Implement ambient `MONGODB_URI` / `MONGODB_PASSWORD` fallback resolution when `--mongodb-uri` is omitted from CLI in `src/main.rs`
- [X] T012 [US2] Enforce fail-fast credential checks in `Production` profile prior to any storage connection or directory walk in `src/main.rs`

**Checkpoint**: User Stories 1 and 2 functional — secrets completely externalized and validated per profile.

---

## Phase 5: User Story 3 - Repository & Documentation Sanitization (Priority: P3)

**Goal**: Purge all hardcoded plaintext passwords across all codebase tests, documentation, and specs.

**Independent Test**: Run repository-wide text search confirming zero occurrences of historical password `pwd`.

### Tests for User Story 3

- [X] T013 [P] [US3] Sanitize `tests/unit/mongodb_config_tests.rs` and `tests/integration/mongodb_sink_tests.rs` to remove hardcoded passwords and read from environment variables with safe defaults

### Implementation for User Story 3

- [X] T014 [P] [US3] Sanitize `README.md` replacing hardcoded passwords with `<MONGODB_PASSWORD>` and documenting profiles and environment variables
- [X] T015 [P] [US3] Sanitize historical markdown files (`specs/005-mongodb-storage-sink/quickstart.md`, `specs/005-mongodb-storage-sink/contracts/mongodb-cli-contract.md`, `specs/005-mongodb-storage-sink/plan.md`, `specs/005-mongodb-storage-sink/spec.md`)
- [X] T016 [US3] Add automated security audit test in `tests/unit/profile_config_tests.rs` asserting no hardcoded secret patterns leak in logs or string representations

**Checkpoint**: All user stories functional — zero plaintext secrets remain across the codebase.

---

## Phase 6: Polish & Cross-Cutting Concerns

**Purpose**: Test registration, documentation, and quality gates

- [X] T017 Register `profile_config_tests.rs` in `tests/unit.rs`
- [X] T018 Run automated quality gates: `cargo check`, `cargo test`, `cargo fmt --check`, and `cargo clippy --all-targets -- -D warnings`
- [X] T019 Verify quickstart scenarios per `specs/006-config-profiles-secrets/quickstart.md`

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
│Phase 2: Foundational (T003-T005)│
└────────────────┬────────────────┘
                 │
                 ▼
┌─────────────────────────────────┐
│Phase 3: US1 Profile Execution   │
│ (T006-T008)                     │
└────────────────┬────────────────┘
                 │
                 ▼
┌─────────────────────────────────┐
│Phase 4: US2 Externalize Secrets │
│ (T009-T012)                     │
└────────────────┬────────────────┘
                 │
                 ▼
┌─────────────────────────────────┐
│Phase 5: US3 Repo Sanitization   │
│ (T013-T016)                     │
└────────────────┬────────────────┘
                 │
                 ▼
┌─────────────────────────────────┐
│   Phase 6: Polish (T017-T019)   │
└─────────────────────────────────┘
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)
1. Complete Phase 1: Setup (`T001`–`T002`).
2. Complete Phase 2: Foundational (`T003`–`T005`).
3. Complete Phase 3: User Story 1 (`T006`–`T008`).
4. **STOP and VALIDATE**: Verify `metadata-extractor --profile local` and `--profile production`.

### Incremental Delivery
1. Foundation & Abstractions (`T001`–`T005`)
2. MVP Profile Execution (`T006`–`T008`)
3. Secret Externalization & Dynamic Injection (`T009`–`T012`)
4. Full Codebase & Documentation Scrubbing (`T013`–`T016`)
5. Quality Gates & Verification (`T017`–`T019`)
