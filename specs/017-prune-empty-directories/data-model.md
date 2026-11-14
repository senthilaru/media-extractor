# Data Model: Prune Empty Directories Cleanup

## Entities

### 1. `PruneExecutionReport` (Updated)

Represents the execution outcome of a duplicate pruning operation on a targeted drive or directory.

| Field | Type | Description |
|---|---|---|
| `drive_id` | `String` | Identifier of target drive or path |
| `mount_path` | `PathBuf` | Mount path or target root path |
| `is_dry_run` | `bool` | True if run in dry-run preview mode |
| `total_candidates` | `usize` | Total files marked for removal |
| `verified_masters` | `usize` | Candidates with verified canonical master |
| `unverified_masters` | `usize` | Candidates skipped due to missing master |
| `files_deleted` | `usize` | Count of duplicate files physically removed |
| `records_purged` | `usize` | Count of MongoDB records purged |
| `bytes_reclaimed` | `u64` | Total bytes freed on disk |
| `empty_directories_removed` | `usize` | Count of empty directories deleted |
| `empty_directories_eligible` | `usize` | Count of empty directories eligible in dry-run |
| `errors` | `Vec<String>` | Diagnostic warnings or non-fatal errors |

### 2. `EmptyDirectoryCleaner` (Domain Helper / Workflow Component)

Encapsulates upward directory traversal logic and boundary constraints.

- **Inputs**:
  - `prune_root`: Target root directory boundary (`&Path`). Traversal stops here; this path is never deleted.
  - `affected_file_paths`: List of files that were deleted (or planned for deletion in dry-run) (`&[PathBuf]`).
  - `guard`: `&ArchiveProtectionGuard` preventing modification of master archives.
  - `execute`: `bool` indicating whether to physically delete or simulate.
- **Outputs**:
  - `DirectoryCleanupOutcome`: Struct containing `directories_removed: usize` and `errors: Vec<String>`.

## Validation Rules & State Transitions

1. **Root Boundary Invariant**:
   `dir_to_delete.starts_with(prune_root) && dir_to_delete != prune_root`
2. **Archive Safety Invariant**:
   `!guard.is_protected_path(dir_to_delete)`
3. **Emptiness Invariant**:
   `read_dir(dir)` contains 0 entries, OR contains only entries whose file names match recognized disposable OS files (`.DS_Store`, `Thumbs.db`).
4. **Bottom-Up Ordering**:
   Directories must be evaluated and removed in descending depth order (deepest path first) to enable ancestor collapse.
