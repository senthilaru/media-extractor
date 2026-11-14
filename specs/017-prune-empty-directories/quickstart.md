# Quickstart: Pruning Empty Directories

## Verification Scenario 1: Preview Empty Directory Removal (Dry-Run)

1. Ensure duplicate files exist in a nested subdirectory inside a staging or unorganized directory.
2. Run duplicate pruning without `--execute`:
   ```bash
   cargo run -- dedup prune --drive /Volumes/SP-1/SPSA/unorganized/Photos
   ```
3. Verify output includes `Empty Directories Eligible: <N>`.
4. Check on disk that no files and no directories were modified or deleted.

## Verification Scenario 2: Execute Pruning and Clean Empty Directories

1. Execute pruning:
   ```bash
   cargo run -- dedup prune --drive /Volumes/SP-1/SPSA/unorganized/Photos --execute
   ```
2. Verify output includes:
   ```text
   Files Deleted:                 <N>
   Empty Directories Removed:     <M>
   ```
3. Inspect the filesystem:
   - Duplicate files are deleted.
   - Any subdirectories that held only duplicate files are gone.
   - The root `/Volumes/SP-1/SPSA/unorganized/Photos` remains present.
   - Any subdirectories containing remaining non-duplicate files remain present.

## Verification Scenario 3: Automated Integration Test Suite

Run unit and integration tests verifying directory cleanup, root protection, and boundary handling:
```bash
cargo test --test test_drive_pruner
```
