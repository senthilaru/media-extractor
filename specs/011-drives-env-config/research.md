# Research: Sample Drive Configuration and Environment-Driven Drive Declarations

## Architectural Decisions & Research Findings

### 1. Environment Variable Parsing & Interpolation Strategy

- **Decision**: Implement a regex/lexer-based variable interpolator supporting `${VAR}` and `${VAR:-default}` syntax directly within `DrivesConfigLoader`, applied to YAML/JSON string contents before serialization into `DriveConfiguration`.
- **Rationale**:
  - Consistent with 12-Factor principles and existing credential interpolation in `DatabaseSinkConfig`.
  - Allows users to maintain a single portable `drives.yaml` across multiple operating systems (macOS `/Volumes/...`, Linux `/mnt/...`, Windows `D:\...`) by referencing ambient environment variables.
  - Using `${VAR:-fallback}` allows safe defaults when specific environment variables are unset.
- **Alternatives Considered**:
  - *External templating engine (tera / handlebars)*: Adds heavyweight dependencies for simple string substitutions.
  - *Environment-only override*: Fails to satisfy users who want a structured YAML file with dynamic mount path placeholders.

---

### 2. Direct Environment Variable Drive Synthesis (`.env` Fallback)

- **Decision**: Provide `DrivesConfigLoader::load_from_env_or_fallback(Option<&Path>)`. When no explicit `--config` is passed and `drives.yaml` does not exist on disk, scan the environment for `DRIVE_PRIMARY_*` and `DRIVE_REPLICA<N>_*` variables to automatically assemble a typed `DriveConfiguration`.
- **Priority & Group Mapping Conventions**:
  - `DRIVE_PRIMARY_MOUNT`: Group `primary`, Priority `100`, ID `primary_main`.
  - `DRIVE_PRIMARY_MASTER`: Canonical master directory path on the primary drive. Defaults to `<DRIVE_PRIMARY_MOUNT>/SPSA/media/master` if omitted.
  - `DRIVE_REPLICA1_MOUNT` / `DRIVE_REPLICA1_MASTER`: Group `replica1`, Priority `80`, ID `replica_1`.
  - `DRIVE_REPLICA2_MOUNT` / `DRIVE_REPLICA2_MASTER`: Group `replica2`, Priority `70`, ID `replica_2`.
  - Dynamically supports arbitrary replica indices (`1..=10`).
- **Rationale**:
  - Minimizes friction: users who already have `.env` for `MONGODB_URI` can simply add `DRIVE_PRIMARY_MOUNT=/Volumes/SP-1` and run `metadata-extractor dedup` or `metadata-extractor ingest` immediately without touching or creating any YAML file.
- **Alternatives Considered**:
  - *Hardcoding drive IDs in source code*: Rejected by Principle VI and SOLID principles.
  - *Requiring YAML file always*: Inconvenient for Docker/containerized or automated CI workflows where passing environment variables is the cloud standard.

---

### 3. Master Directory Fallback Heuristics

- **Decision**: If `DRIVE_<GROUP>_MOUNT` is supplied without an explicit `DRIVE_<GROUP>_MASTER`, the system defaults the master path to `<mount_path>/SPSA/media/master`. If the subdirectory `<mount_path>/media/master` exists, it prefers that.
- **Rationale**:
  - Avoids repetitive `.env` configuration when following the project's recommended folder conventions.
- **Alternatives Considered**:
  - *Defaulting master path to the root mount path*: Disallowed because mixing media master temporal hierarchy (`2021/30-Nov-2021/...`) directly at the drive root can lead to collisions with drive system folders (`.Trashes`, `System Volume Information`).

---

### 4. Zero Hardcoded Credentials & Masking Compliance (Constitution VI)

- **Decision**: Ensure that error messages and debug outputs involving environment variables do not expose sensitive environment data, and ensure `drives.example.yaml` contains only dummy paths (e.g. `/Volumes/DummyPrimary-1`, `/Volumes/DummyReplica-1`).
- **Rationale**:
  - Strict compliance with Constitution Principle VI (Zero Hardcoded Credentials) and Principle IV (Idiomatic Rust & Zero-Cost Reliability).

---

### 5. CLI Short Flag Disambiguation

- **Decision**: Remove `short = 'c'` from `channel_capacity` in `DedupArgs`, keeping `-c` exclusively mapped to `--config`.
- **Rationale**:
  - In `clap`, multiple fields on the same struct declaring `short = 'c'` result in the latter field overriding the former. By making `--channel-capacity` long-only on `dedup`, `-c <CONFIG>` works unambiguously.

---

### 6. Uniform MongoDB Secret Interpolation

- **Decision**: Centralize MongoDB connection URI building into `resolve_db_config(cli_uri, cli_collection)` in `src/main.rs`, ensuring that `<MONGODB_PASSWORD>` is consistently replaced with `MONGODB_PASSWORD` across all subcommands (`dedup`, `prune`, `ingest`).
- **Rationale**:
  - Prevents SCRAM `AuthenticationFailed` errors when users run subcommands with `.env` containing placeholder URI strings.

---

### 7. BSON 64-Bit Integer Two's-Complement Reinterpretation

- **Decision**:
  - In `bson_to_json`: Reinterpret negative `Bson::Int64(i)` values where `i < 0` back to `(i as u64)` before emitting `serde_json::Value::Number`.
  - In `MediaFingerprint` and `RotatedHashes`: Add custom deserializers (`deserialize_u64_or_signed` and `deserialize_u64_or_signed_opt`) that accept both unsigned `u64` and signed `i64` integers.
- **Rationale**:
  - The BSON specification lacks an unsigned 64-bit integer type. Perceptual hashes (`u64`) with the high bit set are stored in BSON as negative `Int64`. Reinterpreting them on deserialization prevents `invalid value: integer -..., expected u64` errors.
