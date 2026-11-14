# Phase 0 Research: Archive Immutability & Deletion Protection

## Technical Decisions

### Decision 1: Dedicated Immutability Guard Component (`ArchiveProtectionGuard`)
- **Decision**: Create `ArchiveProtectionGuard` to serve as the single, authoritative gate for validating file deletion requests across the application.
- **Rationale**:
  - Centralizing deletion checks prevents different services (`DrivePruningService`, `MasterIngestService`) from having divergent or incomplete safety logic.
  - Guarantees defense-in-depth: pruning drive validation (startup), manifest candidate filtering (planning), and runtime removal circuit breakers (execution).
- **Alternatives Considered**:
  - *Scattered ad-hoc `if drive.group != "primary"` checks*: Fragile; prone to omission in new features or CLI options.
  - *Relying only on filesystem permissions (chmod)*: Brittle across OS environments (macOS, Linux), external USB/exFAT/NTFS drives that do not support POSIX permission bits, or when the process runs as an administrative user.

### Decision 2: Canonical Path Matching & Symlink Resolution
- **Decision**: Use `std::fs::canonicalize` on both candidate paths and protected drive roots, falling back to clean relative normalization when files or directories are virtual or not yet mounted.
- **Rationale**:
  - Symlinks, relative traversal segments (`..`), or differing path casing/trailing slashes could otherwise bypass simple string prefix matching.
  - Canonicalizing both sides of the comparison guarantees that `/Volumes/SP-1/SPSA/media/master/../master/photo.jpg` is recognized as inside `/Volumes/SP-1/SPSA/media/master`.
- **Alternatives Considered**:
  - *Raw string prefix comparison (`starts_with`)*: Can be bypassed by symlinks, relative segments, or trailing slash inconsistencies.

### Decision 3: Fail-Fast Startup Abort on Pruning Commands
- **Decision**: In `dedup prune`, evaluate the target drive identifier at the very beginning of `generate_manifest` and `prune_drive`. If the drive group is `"primary"` or starts with `"replica"`, or if the resolved path falls within any primary or replica mount/master root, abort immediately with `DeduplicationError::PruneForbiddenOnProtectedArchive`.
- **Rationale**:
  - Operators should immediately receive feedback that primary and replica drives cannot be pruned, without waiting for database queries or manifest generation.
  - Completely eliminates the chance of accidental `--execute` on primary archives.
- **Alternatives Considered**:
  - *Allowing pruning on replica drives*: Rejected. Replicas are permanent backup mirrors; duplicate pruning on replicas would destroy verified backup redundancy.

### Decision 4: Safe Deletion Wrapper (`safe_delete_file`)
- **Decision**: Expose `ArchiveProtectionGuard::safe_delete_file(path, config)` which checks immutability immediately before calling `std::fs::remove_file(path)`.
- **Rationale**:
  - Provides a hard circuit breaker right at the point of file deletion. Even if a bug in the database marks a primary master file as `MarkForRemoval`, the circuit breaker will intercept and prevent deletion.
- **Alternatives Considered**:
  - *Calling `std::fs::remove_file` directly with manual checks*: Easy to forget check in future development.
