# Feature Specification: Purge Obsolete Database Records on Duplicate Prune

**Feature Branch**: `015-purge-pruned-records`

**Created**: 2026-11-12

**Status**: Draft

**Input**: User description: "when I run this command 'cargo run -- dedup prune --drive /Users/spsenthil/Documents/spsa/samples --execute' it deleted the files, but didnt delete the records from the database, its simply marked as 'Removed', as the files are copied to the master/primary, we can delete this entry, its obsolete."

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Purge Database Records for Pruned Duplicate Files (Priority: P1) 🎯 MVP

As an archive manager running `dedup prune --execute`, when redundant duplicate files on a staging or external drive are deleted from disk after verified master copy ingestion, I want their corresponding database metadata records to be completely deleted from the database rather than lingering as obsolete records with status `Removed`, so that my database index contains only active, existing media assets.

**Why this priority**: Core user requirement. Once duplicate files are confirmed to exist safely in the canonical master archive and their physical duplicates are pruned from external/staging drives, keeping obsolete records adds database clutter, index bloat, and confusion in search queries and cluster evaluations.

**Independent Test**:
Can be fully tested by inserting duplicate candidates in the database whose physical copies exist on a staging drive and master copy exists on the primary drive. Execute `dedup prune --drive <STAGING_DRIVE> --execute`. Verify that the duplicate files are deleted from disk, and their records are completely deleted from the database (querying by their document IDs returns 0 records), while the canonical master document remains intact in the database.

**Acceptance Scenarios**:

1. **Given** a duplicate file on a staging drive that has been copied to the primary master, **When** `dedup prune --drive <path> --execute` runs and deletes the file from disk, **Then** its metadata record is deleted from the database.
2. **Given** a duplicate document whose file is already missing from disk (e.g., previously deleted), **When** `dedup prune --drive <path> --execute` processes the candidate, **Then** the obsolete database record is deleted from the database.
3. **Given** a canonical master document and its corresponding file on the primary drive, **When** duplicate pruning runs on any drive, **Then** the canonical master document is NEVER deleted or modified.

---

### User Story 2 - Purge Pre-Existing "Removed" Records on Target Drive (Priority: P2)

As a database administrator who previously ran `dedup prune` without record purging, when I run `dedup prune --drive <path> --execute`, I want the command to also purge any obsolete records already sitting in the database with status `Removed` for the target drive, cleaning up residual historical debris.

**Why this priority**: Resolves existing dirty state from previous pruning runs where records were marked `Removed` instead of deleted.

**Independent Test**:
Insert documents into the database with `dedup.status = "Removed"` belonging to the target drive. Execute `dedup prune --drive <path> --execute`. Verify that those obsolete records are purged from the database, and the execution report includes them in the count of purged records.

**Acceptance Scenarios**:

1. **Given** existing records in the database with status `Removed` on the target drive, **When** `dedup prune --drive <path> --execute` executes, **Then** those obsolete records are removed from the database.
2. **Given** `dedup prune --drive <path>` run in dry-run mode (without `--execute`), **When** candidate evaluation runs, **Then** neither files nor database records are deleted, and the summary displays the count of records that would be purged.

---

### User Story 3 - Telemetry & Audit Visibility for Purged Records (Priority: P3)

As a CLI user, when pruning completes, I want clear, visible console output and execution telemetry reporting the exact number of database records purged, so that I have complete confidence in what was cleaned up.

**Why this priority**: Observability and transparency. Operators need to know how many files were deleted and how many database records were purged.

**Independent Test**:
Run `dedup prune --drive <path> --execute` against a drive with 5 duplicate files. Verify the console output displays "Database Records Purged: 5" along with reclaimed byte statistics.

**Acceptance Scenarios**:

1. **Given** a pruning run with 5 deleted duplicates, **When** execution finishes, **Then** the console summary and JSON manifest report `records_purged: 5`.
2. **Given** a dry-run pruning preview, **When** preview finishes, **Then** the summary reports `Records to Purge: 5` and explicitly states `[DRY-RUN] No records were deleted from the database`.

---

## Edge Cases

- **What if database deletion fails for a document after physical file deletion?**
  - The system logs an error for that specific record, increments the error tally, and continues processing remaining tasks without crashing the entire batch.
- **What if the target drive is a primary or replica drive?**
  - Strict archive immutability rules prevent `dedup prune` from running at all; it fails fast before any file or database record deletion can occur.
- **What if a record's master verification failed?**
  - Unverified files are skipped: neither the physical file on disk nor the database record is deleted.
- **What if network or database connectivity drops mid-batch?**
  - Successfully deleted records remain purged; unreached records remain untouched. The command outputs an error and appropriate exit code.

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST provide the capability to hard-delete metadata documents from the database by their unique document ID.
- **FR-002**: During `dedup prune --execute`, when a duplicate file with a verified master is pruned from disk, the system MUST delete its corresponding metadata document from the database.
- **FR-003**: During `dedup prune --execute`, if a candidate duplicate file is already missing from disk but has a verified master, the system MUST delete its obsolete metadata document from the database.
- **FR-004**: During `dedup prune --execute`, the system MUST purge any pre-existing records on the target drive that have deduplication status `Removed`.
- **FR-005**: In dry-run mode (`dedup prune` without `--execute`), the system MUST NOT delete any metadata records from the database and MUST report the count of records that would be purged.
- **FR-006**: The system MUST NEVER delete database records belonging to `CanonicalMaster` or `ReplicaKeep` assets under any pruning operation.
- **FR-007**: The pruning execution report and terminal summary MUST display the count of database records purged alongside files deleted and bytes reclaimed.

---

### Key Entities

- **MediaMetadataDocument**: Represents the stored metadata for a file in the database. When pruned as an obsolete duplicate, this document is deleted from the collection.
- **PruneExecutionReport**: Execution outcome model containing `records_purged` (number of database records deleted), `files_deleted`, `bytes_reclaimed`, and error tallies.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of pruned duplicate files with verified canonical masters have their metadata records completely deleted from the database upon `--execute`.
- **SC-002**: Querying the database for documents belonging to deleted duplicate files after prune execution returns zero matches.
- **SC-003**: Zero canonical master or replica records are deleted from the database during any pruning run.
- **SC-004**: In dry-run mode, exactly 0 database records are deleted and 0 files are removed from disk.
- **SC-005**: CLI output clearly shows the number of database records purged and space reclaimed.

---

## Assumptions

- Obsolete duplicate files that have been safely ingested into the primary canonical master archive do not need historical tombstone records in the database.
- Physical file deletion and database record deletion should occur in sequence: physical file verified and deleted first, then database record deleted.
- If a user wants to audit what was deleted, the existing `--manifest-out <PATH>` option already saves the full removal manifest with all document IDs, file paths, hashes, and sizes to disk before deletion.
