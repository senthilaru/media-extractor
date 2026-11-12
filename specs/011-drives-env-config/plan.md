# Implementation Plan: Sample Drive Configuration and Environment-Driven Drive Declarations

**Branch**: `011-drives-env-config` | **Date**: 2026-11-09 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `/specs/011-drives-env-config/spec.md`

## Summary

Provide a committed, production-ready sample template (`drives.example.yaml`) demonstrating dummy drives and canonical master directories for primary, replica1, replica2, and staging drives. Extend `DrivesConfigLoader` to support:
1. Environment variable interpolation (`${VAR}` and `${VAR:-default}`) within YAML/TOML configuration files.
2. Direct dynamic synthesis of `DriveConfiguration` from `.env` environment variables (`DRIVE_PRIMARY_*`, `DRIVE_REPLICA<N>_*`) when no YAML file is specified.
3. Update `.env.example`, `README.md`, and `apis.md` to document drive configuration via environment variables.

---

## Technical Context

**Language/Version**: Rust 1.82+ (2021 Edition)

**Primary Dependencies**: `serde`, `serde_yaml`, `serde_json`, `dotenvy`, `clap`

**Storage**: Local filesystem (`drives.example.yaml`, `.env`, `.env.example`)

**Testing**: `cargo test`, `tests/unit/drives_config_tests.rs`

**Target Platform**: macOS (Apple Silicon M-series optimized), Linux x86_64/arm64

**Project Type**: CLI application & reusable library crate

**Performance Goals**: Drive configuration resolution and interpolation in $< 5$ ms

**Constraints**: Bounded memory, zero credential leakage, strict schema validation, backwards compatible with existing `drives.yaml`

**Scale/Scope**: Arbitrary replica count ($N \in [1..=10]$), multiple drive groups

---

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Check | Status |
|---|---|---|
| **I. High-Throughput I/O & Concurrency** | Configuration loading is non-blocking and executes in single-digit milliseconds during CLI startup. | **PASS** |
| **II. DDD & SOLID Boundaries** | Configuration loading lives in `src/infrastructure/drives_config.rs` and returns clean domain `DriveConfiguration` entities. | **PASS** |
| **III. Measurability & Observability** | Config loading emits clear tracing diagnostics upon resolving drives from file or `.env`. | **PASS** |
| **IV. Idiomatic Rust & Reliability** | Zero `unwrap()` in production paths; typed error handling via `DeduplicationError::ConfigError`. | **PASS** |
| **V. Fault Isolation** | Missing or malformed drive configurations produce actionable validation errors without panics. | **PASS** |
| **VI. Zero Hardcoded Credentials** | All sample files use dummy placeholder paths (`/Volumes/DummyPrimary-1`); real mount points externalized via `.env`. | **PASS** |

---

## Project Structure

### Documentation (this feature)

```text
specs/011-drives-env-config/
├── plan.md              # Implementation plan
├── research.md          # Architectural decisions & interpolation design
├── data-model.md        # Domain models & environment mapping matrix
├── quickstart.md        # Runnable verification scenarios
├── contracts/           # File, env, and Rust API contracts
│   └── drives-env-contract.md
└── checklists/
    └── requirements.md  # Quality validation checklist
```

### Source Code (repository root)

```text
drives.example.yaml             # Committed sample drive configuration template
.env.example                    # Added DRIVE_PRIMARY_* and DRIVE_REPLICA* variables

src/
├── domain/
│   └── dedup_models.rs         # Existing DriveConfiguration, DriveGroup, DriveDefinition
├── infrastructure/
│   └── drives_config.rs        # Extended with env interpolation and synthesize_from_env
└── main.rs                     # Updated dedup and ingest to use load_from_env_or_path

tests/
└── unit/
    └── drives_config_tests.rs  # Unit tests for interpolation, env synthesis, and dummy template
```

**Structure Decision**: Integrated directly into `src/infrastructure/drives_config.rs` and wired through `src/main.rs`.

---

## Complexity Tracking

*No constitutional violations. Design uses standard library string operations and existing serde integrations without additional external crates.*
