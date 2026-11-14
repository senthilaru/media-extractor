# Phase 1 Data Model: Archive Immutability & Deletion Protection

## Entities & Errors

### 1. `DeduplicationError::PruneForbiddenOnProtectedArchive`

Added to `DeduplicationError` in `src/domain/errors.rs`:

```rust
#[derive(Debug, thiserror::Error)]
pub enum DeduplicationError {
    // ... existing variants ...

    #[error("Pruning is strictly forbidden on protected archive storage '{drive_id}' ({path}): {reason}")]
    PruneForbiddenOnProtectedArchive {
        drive_id: String,
        path: PathBuf,
        reason: String,
    },
}
```

---

### 2. `ArchiveProtectionGuard` (Domain Service)

Located in `src/infrastructure/archive_guard.rs` (or `src/domain/archive_guard.rs`):

```rust
pub struct ArchiveProtectionGuard;

impl ArchiveProtectionGuard {
    /// Evaluates if a given filesystem path is located within any protected Primary Master
    /// or Replica drive tree (mount_path or master_path).
    pub fn is_protected_path(path: &Path, config: &DriveConfiguration) -> bool;

    /// Validates whether a target drive is permissible for duplicate pruning.
    /// Returns Err if the drive belongs to group 'primary' or 'replica*'.
    pub fn validate_prune_target(
        drive: &DriveDefinition,
        config: &DriveConfiguration,
    ) -> Result<(), DeduplicationError>;

    /// Executes bitwise-guarded deletion of a file, verifying it does NOT reside in
    /// any protected archive root before invoking filesystem deletion.
    pub fn safe_delete_file(
        path: &Path,
        config: &DriveConfiguration,
    ) -> Result<(), DeduplicationError>;
}
```

---

## Defense-in-Depth Layer Architecture

```mermaid
flowchart TD
    A["CLI Command: dedup prune / ingest --delete-source"] --> B["Layer 1: Drive Validation Check (validate_prune_target)"]
    B -->|Forbidden (Primary or Replica)| E["Abort: DeduplicationError::PruneForbiddenOnProtectedArchive"]
    B -->|Allowed (Staging / Scratch)| C["Layer 2: Manifest Generation Candidate Filter"]
    C -->|Candidate in Primary or Replica| F["Disqualify Candidate (Skipped & Preserved)"]
    C -->|Candidate on Staging| D["Layer 3: Runtime Deletion Circuit Breaker (safe_delete_file)"]
    D -->|Target in Primary or Replica| G["Circuit Breaker: File Protected & Preserved"]
    D -->|Target on Staging| H["Execute: std::fs::remove_file"]
```
