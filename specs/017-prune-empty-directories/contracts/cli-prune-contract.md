# CLI Contract: Prune Empty Directories Cleanup

## Command Invocation

```bash
# Preview duplicate pruning and empty directory cleanup (dry-run)
cargo run -- dedup prune --drive <DRIVE_ID_OR_PATH> [-c <CONFIG>]

# Execute duplicate pruning and empty directory cleanup
cargo run -- dedup prune --drive <DRIVE_ID_OR_PATH> --execute [-c <CONFIG>]
```

## Terminal Output Contracts

### 1. Dry-Run Preview Mode

```text
[DRY-RUN] Pruning preview for drive: staging_drive (/Volumes/SP-1/SPSA/unorganized/Photos)
Candidates Marked for Removal: 12
Verified Canonical Masters:    12
Unverified Masters (Skipped):  0
Reclaimable Space:             145620400 bytes (0.14 GB)
Empty Directories Eligible:    4 (Would be removed upon execution)
Run with --execute to permanently delete duplicate files, clean empty folders, and purge database records.
```

### 2. Execution Mode

```text
[EXECUTE] Pruning completed for drive: staging_drive (/Volumes/SP-1/SPSA/unorganized/Photos)
Files Deleted:                 12
Empty Directories Removed:     4
Records Purged from MongoDB:   12
Reclaimed Space:               145620400 bytes (0.14 GB)
Audit Manifest:                None
Prune Status:                  SUCCESS
```

## Safety Guarantees

1. The target root directory itself (e.g. `/Volumes/SP-1/SPSA/unorganized/Photos`) will NEVER be removed under any conditions.
2. Canonical master directories on primary and replica drives will abort with an error before any pruning operation.
3. Only directories that become strictly empty (or contain only disposable OS metadata files like `.DS_Store`) will be deleted.
