# Feature Specification: Primary and Replica Archive Immutability & Deletion Protection

**Feature Branch**: `014-protect-primary-replicas`

**Created**: 2026-11-12

**Status**: Draft

**Input**: User description: "Never delete primary, replica paths and files, in any stage, to protect from any buggy code"

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Immutable Archive Guard for Primary and Replica Storage (Priority: P1) 🎯 MVP

As a media curator and archive owner, I want the system to enforce an absolute, non-bypassable immutability guard across all stages and operations, so that no file or directory located within any Primary Master path or Replica path can ever be deleted by any command or code path under any circumstances.

**Why this priority**: Core safety guarantee. Primary master storage and replica backup storage hold the authoritative, irreplaceable media collection. Even if software bugs, database corruption, or flawed business logic occur, archive files must be physically protected from deletion.

**Independent Test**: Can be tested by invoking low-level deletion handlers and high-level CLI commands targeting files within configured primary and replica directory trees. In every case, the system must immediately reject the deletion with a descriptive protection error, leaving 100% of the files untouched on disk.

**Acceptance Scenarios**:

1. **Given** a media file located inside a Primary Master folder hierarchy, **When** any deletion method, pruning command, or ingest cleanup routine is executed against that file, **Then** the deletion is blocked before file system I/O, the file remains intact, and a critical protection error is logged.
2. **Given** a media file located inside any configured Replica drive hierarchy, **When** any deletion method, pruning command, or ingest cleanup routine is executed against that file, **Then** the deletion is blocked before file system I/O, the file remains intact, and a critical protection error is logged.
3. **Given** a database record mistakenly marked with status `MarkForRemoval` whose physical path resides on a primary or replica drive, **When** pruning manifest generation evaluates the candidate, **Then** the candidate is disqualified from deletion, flagged as a protected archive asset, and skipped.

---

### User Story 2 - Fail-Fast Protection for Drive Pruning Commands (Priority: P1)

As an operator running `dedup prune`, when I accidentally supply the drive identifier or mount path of a primary master drive or a replica backup drive, I want the command to immediately abort before generating any removal manifest or touching the filesystem, warning me that pruning is permanently disallowed on primary and replica volumes.

**Why this priority**: Prevents catastrophic operator error. If an operator accidentally executes `dedup prune --drive primary` or `dedup prune --drive <replica_path>`, the system must refuse to run immediately.

**Independent Test**: Execute `cargo run -- dedup prune --drive <PRIMARY_OR_REPLICA_ID>` and verify that the command fails immediately with an exit code indicating forbidden operation, without querying database candidates or writing any manifests.

**Acceptance Scenarios**:

1. **Given** the configured Primary Master drive, **When** an operator executes `dedup prune` targeting that drive by ID or path, **Then** the command fails fast with a message stating that pruning is strictly forbidden on primary master storage.
2. **Given** any configured Replica drive, **When** an operator executes `dedup prune` targeting that drive by ID or path, **Then** the command fails fast with a message stating that pruning is strictly forbidden on replica storage.
3. **Given** an unconfigured path that resolves inside a primary or replica directory tree, **When** `dedup prune` is invoked with that path, **Then** path resolution detects that it lies within a protected archive root and halts execution immediately.

---

### User Story 3 - Ingestion Source Deletion Circuit Breaker (`--delete-source`) (Priority: P2)

As an archivist running `ingest --delete-source` to clean up temporary camera cards or staging folders after copying to master, I want the ingestion service to check every file against the immutability guard before attempting deletion, so that files from primary or replica drives are never deleted even if `--delete-source` is passed.

**Why this priority**: Avoids accidental source deletion when ingesting between archive tiers or if an operator mistakenly points `--source-drive` to a replica drive or primary master subfolder.

**Independent Test**: Run `ingest --source-drive <REPLICA_OR_PRIMARY_PATH> --delete-source`. Verify that all files on the primary/replica drive are preserved with zero deletions, and the command warns that source files on protected archive storage cannot be removed.

**Acceptance Scenarios**:

1. **Given** an ingestion task configured with `--delete-source`, **When** the source file path is evaluated, **Then** the system checks whether the path resides within any primary or replica root.
2. **Given** a source file that resides on a primary or replica drive, **When** the copy completes, **Then** the system refuses to delete the source file and retains it intact.
3. **Given** a source file on an external staging drive or scratch directory, **When** the copy completes and passes bitwise verification, **Then** the staging file is safely deleted as requested.

---

### User Story 4 - Canonicalized Symlink and Traversal Protection (Priority: P3)

As a security-conscious administrator, I want archive protection checks to resolve real canonical filesystem paths (dereferencing symbolic links and resolving `..` parent segments), so that symlinks or aliased mount paths cannot be used to circumvent archive deletion protection.

**Why this priority**: Defense-in-depth against subtle path aliasing or symlinks that could otherwise fool naive string-prefix checks.

**Independent Test**: Create a symlink in a temporary directory pointing to a file in the primary master directory. Attempt to prune or delete through the symlink path. Verify that the system resolves the canonical target, identifies it as belonging to the primary archive, and rejects the deletion.

**Acceptance Scenarios**:

1. **Given** a symlink pointing to a primary master or replica asset, **When** a deletion operation targets the symlink, **Then** canonicalization resolves the real destination path and blocks deletion.
2. **Given** relative path syntax (e.g., `/tmp/../Volumes/Master/...`), **When** path validation evaluates the path, **Then** the normalized path is checked against all protected roots and blocked if inside primary or replica trees.

---

### Edge Cases

- **Drive Configuration Unavailable or Partial**: What happens if `drives.yaml` only specifies some drives or an ad-hoc directory is passed via CLI?
  - *Resolution*: The system inspects all loaded drives (primary and replicas), ambient environment configurations, and destination paths recorded on canonical master documents. Any path marked as a master or replica path in database metadata or in configuration is unconditionally protected.
- **Subdirectories Inside Primary Master (e.g., Scratch or Non-Master Folders)**: What if a folder exists on the primary drive that is outside the canonical master hierarchy?
  - *Resolution*: By default, the entire mount path of the primary drive and all replica drives are protected from pruning unless a drive group explicitly has `allow_pruning: true` (e.g., `staging`, `scratch`). The primary drive and replica drives are strictly marked `allow_pruning: false`.
- **Database Status Corruption**: What happens if a database bug marks a file located on the primary master volume as `MarkForRemoval`?
  - *Resolution*: The deletion safety guard checks the physical filesystem path against primary and replica roots at the exact moment of deletion. Regardless of what the database document says, physical deletion is forbidden and an error is logged.

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST implement a centralized, authoritative path protection guard (`ArchiveProtectionGuard`) responsible for validating whether any filesystem path is eligible for deletion.
- **FR-002**: The path protection guard MUST classify any path residing within the mount path or master path of the Primary drive as strictly protected and immutable (never deletable).
- **FR-003**: The path protection guard MUST classify any path residing within the mount path or master path of any Replica drive as strictly protected and immutable (never deletable).
- **FR-004**: The path protection guard MUST canonicalize all paths (resolving symbolic links and relative segments) prior to evaluating whether they fall within protected archive roots.
- **FR-005**: The `dedup prune` command MUST fail fast at startup if the targeted drive ID or resolved path corresponds to a Primary drive or a Replica drive, exiting with an explicit error without performing scans or generating manifests.
- **FR-006**: During pruning manifest generation, the system MUST inspect candidate file paths against the path protection guard and disqualify any candidate that resolves inside primary or replica storage, even if the database record is marked `MarkForRemoval`.
- **FR-007**: Immediately prior to calling any filesystem deletion routine (e.g., `std::fs::remove_file`), the system MUST re-verify the target path against the path protection guard as a final runtime circuit breaker.
- **FR-008**: During ingestion with `--delete-source`, the system MUST NOT delete the source file if it is located on a Primary or Replica drive, regardless of transfer outcome.
- **FR-009**: The system MUST log an explicit, high-visibility warning whenever a deletion attempt is blocked by the archive protection guard, identifying the path, drive, and reason.

---

### Key Entities *(include if feature involves data)*

- **Archive Protection Guard**: A stateless or configuration-backed domain service that evaluates filesystem paths against known primary master and replica roots to enforce deletion immutability.
- **Protected Drive Specification**: Drive configurations marked with `allow_pruning = false` and associated with groups `primary` or `replica*`, representing permanent archive storage tiers.
- **Immutability Violation Error**: A dedicated, strongly typed error returned whenever an operation attempts to prune, remove, or truncate an asset within primary or replica boundaries.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 0 files on the Primary Master volume are deleted across 100% of test runs and operational workflows, including misconfigured or malicious pruning invocations.
- **SC-002**: 0 files on any configured Replica volume are deleted across 100% of test runs and operational workflows.
- **SC-003**: 100% of deletion attempts targeting primary or replica files (direct path, symlinked path, or mistaken `MarkForRemoval` record) are blocked before file system modification occurs.
- **SC-004**: Running `dedup prune` with `--drive primary` or `--drive replica` aborts in under 50 milliseconds with a clear, user-friendly protection error.
- **SC-005**: All existing valid pruning capabilities on staging and scratch volumes continue to function safely with 0 regressions.

---

## Assumptions

- Staging and scratch drives (e.g., SD cards, camera dumps, temporary folders) are separate from the primary master and replica backup drives.
- The `drives.yaml` configuration and environment variables define the primary master drive and replica drives.
- Physical filesystem write permissions on the archive volumes may permit file creation (for ingestion and replication) while the application software strictly forbids deletion operations within those trees.
