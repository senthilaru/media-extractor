# Quickstart Validation: Purge Obsolete Database Records on Duplicate Prune

## Scenario 1: Dry-Run Preview Shows Records to Purge

### Steps
1. Run `dedup prune` without `--execute`:
   ```bash
   cargo run -- dedup prune --drive /Users/spsenthil/Documents/spsa/samples --config drives.yaml
   ```
2. Verify:
   - Output includes `Database Records to Purge: N`.
   - No files are deleted on disk.
   - All records in MongoDB remain intact.

---

## Scenario 2: Execute Prunes Files and Deletes Database Records

### Steps
1. Run `dedup prune` with `--execute`:
   ```bash
   cargo run -- dedup prune --drive /Users/spsenthil/Documents/spsa/samples --config drives.yaml --execute
   ```
2. Verify:
   - Output displays `Database Records Purged: N`.
   - Target duplicate files are removed from the disk directory.
   - Querying MongoDB for the deleted duplicate document IDs returns `null` (documents completely deleted).
   - Canonical master document remains intact and queryable in MongoDB.
