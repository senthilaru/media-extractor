# Implementation Plan: Externalized Configuration, Profiles, and Secret Management

**Branch**: `006-config-profiles-secrets` | **Date**: 2026-11-08 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/006-config-profiles-secrets/spec.md`

## Summary

This feature externalizes all database connection credentials and secrets, scrubs all hardcoded passwords from the entire repository (code, tests, documentation), and introduces runtime operational profiles (`local` and `production`). Configuration resolution follows a deterministic hierarchy (CLI args $\to$ environment variables $\to$ `.env` files via `dotenvy` $\to$ profile defaults) while enforcing strict fail-fast validation in `production` and sanitizing all logs and error streams.

---

## Technical Context

**Language/Version**: Rust 2021 (Rust 1.80+)  
**Primary Dependencies**:
- `clap = { version = "4.5", features = ["derive", "env"] }` (already present, add `env` feature or parse env vars)
- `dotenvy = "0.15"` (for `.env` / `.env.{profile}` loading)
- `mongodb = "3.2"` (already present)
- `thiserror = "2.0"` (already present)
- `tracing = "0.1"` (already present)

**Storage**: MongoDB 4.4+ (externalized connection, dynamic URI secret interpolation)  
**Testing**: `cargo test` (unit tests and integration tests)  
**Target Platform**: macOS (Apple Silicon M1 optimized), Linux  
**Project Type**: CLI tool & asynchronous batch ingestion pipeline  
**Performance Goals**: Configuration resolution in $< 50$ms; zero runtime overhead during scanning/streaming  
**Constraints**:
- Strictly 100% safe Rust.
- Zero plaintext passwords in repository or logs.
- Fail fast ($< 100$ms) on missing required secrets in `production` profile.

---

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Check | Status |
|---|---|---|
| I. High-Throughput I/O & Concurrency | Configuration resolution happens once at boot prior to scanner traversal; zero impact on stream throughput. | PASS |
| II. Domain-Driven Design & SOLID | `AppProfile` and `AppConfig` reside in domain models; environment/file reading in infrastructure. | PASS |
| III. Comprehensive Observability | All logs mask secrets using `sanitized_uri()`; profile and database telemetry reported on stderr. | PASS |
| IV. Idiomatic Rust & Reliability | Strongly typed `AppProfile` enum and `StorageError::ConfigurationError`; zero `unwrap()` or panics on user input. | PASS |
| V. Fault Isolation | Missing configuration triggers deterministic exit codes (`2`), isolated from internal pipeline errors. | PASS |

---

## Project Structure

### Documentation (this feature)

```text
specs/006-config-profiles-secrets/
├── plan.md              # This file (/speckit-plan command output)
├── research.md          # Phase 0 output (/speckit-plan command)
├── data-model.md        # Phase 1 output (/speckit-plan command)
├── quickstart.md        # Phase 1 output (/speckit-plan command)
├── contracts/           # Phase 1 output (/speckit-plan command)
│   └── cli-profile-contract.md
└── tasks.md             # Phase 2 output (/speckit-tasks command)
```

### Source Code

```text
.env.example                             # Environment variable template with placeholders
src/
├── domain/
│   ├── metadata_models.rs               # AppProfile, DatabaseSinkConfig secret interpolation
│   └── errors.rs                        # ConfigurationError variant in StorageError
├── infrastructure/
│   ├── config_loader.rs                 # Dotenvy loading, profile resolution, secret injection
│   └── mongodb_sink.rs                  # Sanitized connection execution
└── main.rs                              # CLI flags (--profile), startup config resolution
tests/
├── unit/
│   └── profile_config_tests.rs          # Profile parsing, secret interpolation, sanitization tests
└── integration/
    └── mongodb_sink_tests.rs            # Integration tests reading from env vars, zero hardcoded passwords
```

---

## Key Rules
- Never commit actual passwords or credentials.
- In `production` profile, refuse connection if password is placeholder or empty.
- Always mask credentials in any output to stdout or stderr.
