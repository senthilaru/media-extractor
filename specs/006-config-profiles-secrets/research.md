# Research & Technical Decisions: Externalized Configuration, Profiles, and Secret Management

**Feature**: `006-config-profiles-secrets`
**Date**: 2026-11-08
**Status**: Completed

---

## 1. Profile Architecture & Operational Modes

### Decision
Implement a strongly-typed `AppProfile` enum (`Local`, `Production`) in domain models (`src/domain/metadata_models.rs` or `src/domain/config.rs`) selectable via:
1. CLI flag: `--profile <local|production>`
2. Environment variables: `APP_PROFILE` or `MEDIA_PROFILE`
3. Fallback default: `local`

### Rationale
- Decouples operational environments (developer workstation vs. production server/container).
- `Local` mode provides convenient defaults, automatic loading of `.env` or `.env.local`, relaxed credential requirements for development, and descriptive standard error messages.
- `Production` mode enforces zero tolerance for missing credentials, refuses default/sample secrets, strictly validates environment variables, and enables production telemetry.
- Adheres to the 12-Factor App methodology for config-in-the-environment.

### Alternatives Considered
- *Separate binaries*: Heavyweight, violates Hexagonal architecture, causes code drift.
- *Complex YAML/TOML configuration files*: Adds unnecessary runtime parser dependencies and file synchronization overhead when CLI flags and environment variables suffice.

---

## 2. Credential Externalization & Secret Interpolation

### Decision
Externalize secrets using standard environment variables:
- `MONGODB_PASSWORD`: Dedicated password environment variable.
- `MONGODB_URI`: Full or templated connection URI.
- `MONGODB_COLLECTION`: Optional collection override.
- Dynamic password interpolation: If `MONGODB_URI` contains `<MONGODB_PASSWORD>`, `<password>`, or `${MONGODB_PASSWORD}` (or if password is empty in URI `mongodb://user@host:port/db`), interpolate the `MONGODB_PASSWORD` environment variable safely with URL encoding.
- Integrate `dotenvy = "0.15"` to transparently load `.env` and `.env.{profile}` files if present at startup without failing if absent.

### Rationale
- Developers and orchestrators (Kubernetes Secrets, Docker Compose, AWS SSM, GitHub Actions) inject secrets via environment variables.
- URL-encoding interpolated passwords prevents connection breakage when passwords contain special characters like `@`, `:`, `/`, or `%`.
- Loading `.env` via `dotenvy` allows developers to keep credentials out of git history while running seamlessly locally.

### Alternatives Considered
- *Custom file parser for .env*: Fragile and reinvents tested libraries. `dotenvy` is pure Rust, actively maintained, and lightweight.
- *Interactive CLI password prompt*: Impairs batch/CI/CD pipelines and headless daemon execution.

---

## 3. Scrubbing Hardcoded Secrets & Repository Sanitization

### Decision
1. Purge all occurrences of `pwd` from `README.md`, `tests/`, and `specs/005-mongodb-storage-sink/`.
2. Provide a `.env.example` file with sanitized placeholders (`<MONGODB_PASSWORD>`).
3. Update tests to use environment variable `MONGODB_URI` / `MONGODB_PASSWORD` with `dotenvy::dotenv().ok()` so integration tests against local MongoDB pick up user/CI secrets without baking them into Rust code.
4. Add an automated test or sanity check verifying that neither source code nor documentation leaks sensitive credentials.

### Rationale
- Eliminates security vulnerability warnings and prevents real credentials from entering source control.
- Keeps integration tests runnable both locally (via `.env` or pre-set env vars) and in CI pipelines without committing secrets.

---

## 4. Configuration Resolution Precedence

### Decision
Deterministic multi-layer hierarchy:
1. **Command-Line Arguments**: Explicit `--mongodb-uri`, `--collection`, `--profile`.
2. **Environment Variables**: `MONGODB_URI`, `MONGODB_PASSWORD`, `APP_PROFILE`, `MONGODB_COLLECTION`.
3. **Environment Files**: Loaded in order `.env.{profile}` then `.env` (via `dotenvy`).
4. **Profile Defaults**: Default collections, endpoints, and validation rules according to active `AppProfile`.

### Rationale
Follows standard POSIX and cloud-native conventions: CLI flags override ambient environment variables, which override file-based configuration, which override built-in defaults.
