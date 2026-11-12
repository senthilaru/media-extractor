# Feature Specification: Sample Drive Configuration and Environment-Driven Drive Declarations

**Feature Branch**: `011-drives-env-config`

**Created**: 2026-11-09

**Status**: Draft

**Input**: User description: "create a sample drives.yaml with dummy drives, also i should be able to pass it from the env file about primary, replica1,replica2, etc."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Provide Ready-to-Use Sample Drive Configuration Template (Priority: P1)

A user setting up home media deduplication and ingest needs a clear, pre-configured template (`drives.example.yaml`) that demonstrates how primary master drives, replica backup drives, and staging drives are declared with dummy paths and canonical master hierarchy paths.

**Why this priority**: Users need an immediate reference configuration to understand how to map their external disks, drive groups, priorities, and canonical folder patterns without guessing the schema or syntax.

**Independent Test**: Can be tested by copying `drives.example.yaml` to `drives.yaml` and verifying that the configuration parser successfully validates the file structure, groups, priorities, and master paths.

**Acceptance Scenarios**:

1. **Given** a new or existing repository installation, **When** the user inspects the repository root, **Then** a well-documented `drives.example.yaml` exists containing sample declarations for `primary`, `replica1`, `replica2`, and `staging` dummy drives with canonical master paths (`<MasterRoot>/<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}`).
2. **Given** the sample file `drives.example.yaml`, **When** the configuration loader parses the file, **Then** all groups and drives load cleanly without schema errors.

---

### User Story 2 - Environment Variable Configuration via `.env` (Priority: P1)

A user maintaining their media drives in `.env` needs the ability to define primary, replica1, replica2, and additional drives directly via environment variables (such as `DRIVE_PRIMARY_MOUNT`, `DRIVE_PRIMARY_MASTER`, `DRIVE_REPLICA1_MOUNT`, `DRIVE_REPLICA1_MASTER`, `DRIVE_REPLICA2_MOUNT`, `DRIVE_REPLICA2_MASTER`) without manually maintaining a separate YAML file if they prefer pure environment configuration.

**Why this priority**: Directly supports the user's primary workflow where configuration is centralized in `.env` along with database credentials, allowing drive mount paths to change per machine or session seamlessly.

**Independent Test**: Define `DRIVE_PRIMARY_MOUNT`, `DRIVE_PRIMARY_MASTER`, `DRIVE_REPLICA1_MOUNT`, and `DRIVE_REPLICA1_MASTER` in `.env` without a YAML file; verify that `dedup` and `ingest` commands load the configured primary and replica drives correctly.

**Acceptance Scenarios**:

1. **Given** environment variables in `.env` specifying `DRIVE_PRIMARY_MOUNT` and `DRIVE_PRIMARY_MASTER`, **When** running a deduplication or ingest command without an explicit YAML file, **Then** the application automatically synthesizes a valid drive configuration with `primary` group priority 100 and the specified paths.
2. **Given** environment variables for multiple replicas (`DRIVE_REPLICA1_MOUNT`, `DRIVE_REPLICA1_MASTER`, `DRIVE_REPLICA2_MOUNT`, `DRIVE_REPLICA2_MASTER`), **When** loaded, **Then** replica drive groups (`replica1`, `replica2`) are created with decreasing priorities (e.g. 80, 70) and proper master paths.

---

### User Story 3 - Environment Variable Expansion Inside `drives.yaml` (Priority: P2)

A user who prefers using a structured YAML configuration file needs the ability to reference environment variables (e.g. `${PRIMARY_MOUNT_PATH:-/Volumes/DummyPrimary}`) inside their `drives.yaml` so sensitive or machine-specific mount paths can be dynamically interpolated from `.env`.

**Why this priority**: Provides full flexibility combining YAML drive hierarchy with `.env` portability across different workstations or operating systems.

**Independent Test**: Supply a `drives.yaml` containing `${DRIVE_PRIMARY_MOUNT:-/Volumes/Default}`, set `DRIVE_PRIMARY_MOUNT=/Volumes/Custom` in `.env`, and verify that the loaded configuration uses `/Volumes/Custom`.

**Acceptance Scenarios**:

1. **Given** a `drives.yaml` file containing `${VARIABLE:-default}` placeholders, **When** loaded with matching `.env` variables defined, **Then** the placeholders are replaced with the environment variable values.
2. **Given** a `drives.yaml` file containing `${VARIABLE:-default}` placeholders where `VARIABLE` is unset, **When** loaded, **Then** the fallback default value is used.

---

### Edge Cases

- **Missing Primary Drive**: If `.env` defines replica drives but no primary drive is defined and no YAML file is present, the configuration loader must raise a clear, actionable error indicating that a primary drive is required for canonical master operations.
- **Malformed Drive Mount Paths**: If an environment variable defines a blank or non-existent path format, the system must log an informative warning or error rather than crashing silently.
- **Precedence Order**: If both an explicit `--config <PATH>` argument and environment variables are present, the explicit CLI argument takes precedence, with environment variable interpolation applied to the YAML content.
- **Case-Insensitive Environment Prefixes**: System standardizes on `DRIVE_` prefix convention (e.g. `DRIVE_PRIMARY_MOUNT`, `DRIVE_REPLICA1_MOUNT`) to prevent collision with unrelated system environment variables.
- **CLI Flag Disambiguation**: In `dedup` subcommand, `-c` MUST map to `--config`; `--channel-capacity` is long-only to prevent parsing digit errors.
- **BSON 64-Bit Integer Sign Wrapping**: MongoDB BSON only supports signed `Int64`. Unsigned 64-bit integers (`u64`) with the high bit set wrap into negative `i64` in BSON and must be transparently reinterpreted back to `u64` during deserialization.
- **Credential Interpolation Uniformity**: Database connection resolution in `dedup`, `prune`, and `ingest` subcommands must interpolate `<MONGODB_PASSWORD>` from `MONGODB_PASSWORD` identically to the root scan command.

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST provide a committed sample template file `drives.example.yaml` at the repository root illustrating configuration of primary, replica1, replica2, and staging dummy drives.
- **FR-002**: System MUST support loading drive group and drive definitions directly from environment variables / `.env` when no configuration file path is explicitly specified.
- **FR-003**: System MUST recognize standard environment variable conventions for primary drive (`DRIVE_PRIMARY_MOUNT`, `DRIVE_PRIMARY_MASTER`) and assign it priority 100 in the `primary` group.
- **FR-004**: System MUST recognize indexed replica drive conventions (`DRIVE_REPLICA1_MOUNT`, `DRIVE_REPLICA1_MASTER`, `DRIVE_REPLICA2_MOUNT`, `DRIVE_REPLICA2_MASTER`, etc.) and assign appropriate replica groups and priority rankings.
- **FR-005**: System MUST interpolate environment variable placeholders (syntax `${VAR}` and `${VAR:-default}`) within YAML/TOML drive configuration files.
- **FR-006**: System MUST automatically default `master_path` to `<mount_path>/SPSA/media/master` or `<mount_path>/media/master` if a drive mount path is provided in `.env` without an explicit master path.
- **FR-007**: System MUST validate that at least one drive in the `primary` group is defined before executing deduplication evaluations or master ingestion.
- **FR-008**: System MUST document `.env` drive configuration variables in `.env.example` and the documentation files (`README.md`, `apis.md`).
- **FR-009**: System MUST ensure `-c` on `dedup` unambiguously resolves to `--config`, avoiding collision with `--channel-capacity`.
- **FR-010**: System MUST interpolate `<MONGODB_PASSWORD>` across all database-accessing subcommands (`dedup`, `dedup prune`, `ingest`).
- **FR-011**: System MUST deserialize negative signed 64-bit integers stored in BSON into unsigned `u64` values for perceptual hashes without deserialization errors.

### Key Entities

- **DriveConfiguration**: Represents the overall configuration containing drive groups and list of drives.
- **DriveGroup**: Represents a logical tier (e.g., `primary`, `replica1`, `replica2`, `staging`) with an integer retention priority.
- **DriveInstance**: Represents a physical or mounted drive with unique ID, group reference, mount path, and optional master path.
- **EnvironmentDriveSource**: Represents configuration values extracted from process environment variables and `.env` files.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users can initialize their personal drive configuration in under 30 seconds by copying `drives.example.yaml` or adding 2 lines to `.env`.
- **SC-002**: 100% of environment variables matching `DRIVE_PRIMARY_*` and `DRIVE_REPLICA<N>_*` are accurately mapped to functional drive instances without needing a YAML file.
- **SC-003**: Ingestion and deduplication workflows succeed seamlessly whether configured via YAML file, `.env` file, or a combination of both.
- **SC-004**: Configuration loading produces clear diagnostic errors within 50 milliseconds if required primary drive information is missing or corrupted.

---

## Assumptions

- Users have basic familiarity with setting variables in `.env` files.
- Drive group priorities follow the standard model: `primary` (100), `replica1` (80), `replica2` (70), decreasing for subsequent replicas.
- Default canonical master hierarchy pattern remains `<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}`.
- If both `.env` and `drives.yaml` are present, `drives.yaml` takes precedence when explicitly supplied via `--config`, while interpolating any `${VAR}` references from `.env`.
