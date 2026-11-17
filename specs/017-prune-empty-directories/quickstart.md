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

## Verification Scenario 3: Sweep On-Demand Empty Directories and 0-Byte Dummy Files

1. Preview sweep for orphaned empty folders and 0-byte dummy files across a whole drive:
   ```bash
   cargo run -- dedup prune --drive "/Volumes/Seagate Backup Plus Drive" --clean-empty-dirs
   ```
2. Execute sweep and clean up ghost directory trees:
   ```bash
   cargo run -- dedup prune --drive "/Volumes/Seagate Backup Plus Drive" --clean-empty-dirs --execute
   ```
3. Verify that 0-byte corrupt/dummy files are unlinked and all containing empty directories are cleanly removed up to the drive mount point ceiling.

## Verification Scenario 4: Automated Integration Test Suite

Run unit and integration tests verifying directory cleanup, root protection, 0-byte file handling, and boundary handling:
```bash
cargo test --test test_drive_pruner
```
