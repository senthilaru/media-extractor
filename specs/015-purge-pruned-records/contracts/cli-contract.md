# CLI Contract: Duplicate Prune Record Purge

## Command Invocation

```bash
cargo run -- dedup prune --drive <DRIVE_ID_OR_PATH> [--execute] [--config <CONFIG>]
```

## Output Schema

### 1. Dry-Run Output (Default)

```text
[DRY-RUN] Pruning preview for drive: samples (/Users/spsenthil/Documents/spsa/samples)
Candidates Marked for Removal: 4
Verified Canonical Masters:    4
Unverified Masters (Skipped):  0
Reclaimable Space:             223382924 bytes (0.21 GB)
Database Records to Purge:     4

Zero files were deleted. Re-run with `--execute` to delete redundant copies and purge obsolete database records.
```

### 2. Execution Output (`--execute`)

```text
[EXECUTED] Pruned redundant duplicates on drive: samples (/Users/spsenthil/Documents/spsa/samples)
Files Deleted:                 4
Database Records Purged:       4
Storage Space Reclaimed:       223382924 bytes (0.21 GB)
Errors / Warnings encountered: 0

Database records for pruned duplicates were permanently deleted.
```
