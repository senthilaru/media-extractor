# Feature Specification: Master Asset Registration and Replication Lifecycle Tracking

**Feature Branch**: `013-master-indexing-and-replication`

**Created**: 2026-11-12

**Status**: Draft

**Input**: User description: "when i do ingest using these commands one by one "cargo run -- /Users/spsenthil/Documents/spsa/samples; cargo run -- dedup --all; cargo run -- ingest --source-drive /Users/spsenthil/Documents/spsa/samples --config drives.yaml", the files are copied into the master drive, but there is no entry in the mondodb database, where the file is store in the master/primary. Ideally, this entry in the db should be kept as master, and other duplicates can be marked for deletion. Eg. i have the same file 5 copies with different fileName, when i provide the path of this directory, in the series of above commands, create entries, dedup, injest into master, and it has to add new entry of the master path in the primary, and maintains the status as replicated or pending for replication, etc."

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Authoritative Primary Master Database Registration (Priority: P1) 🎯 MVP

As a media collection owner executing the multi-step ingest workflow, when an unmastered media file is transferred from a source directory or external volume into the primary master archive, I want the system to immediately create a dedicated database record representing the physical file stored on the primary master volume, so that the catalog accurately reflects the authoritative master location, metadata, and fingerprint.

**Why this priority**: Without a database record for the file placed in the primary master repository, the catalog loses track of the authoritative master, rendering future deduplication, searching, and replica auditing impossible.

**Independent Test**: Can be tested by running the 3-step workflow on a directory with a single unique media file. After ingestion, query the database to verify a record exists with the destination primary master file path, marked as the canonical master, with full content fingerprint and metadata.

**Acceptance Scenarios**:

1. **Given** a media file scanned into the database and marked for ingestion, **When** the ingestion command completes transferring the file to the primary master drive, **Then** a database record is created or updated representing the primary master file path, with its deduplication status designated as the canonical master.
2. **Given** the master file record created in the database, **When** its attributes are inspected, **Then** it contains the complete file path on the primary master volume, byte size, media category, capture/creation date, and cryptographic content hash.
3. **Given** an ingest operation that fails during file transfer or verification, **When** error handling executes, **Then** no canonical master record is added to the database for that file, preventing ghost catalog entries.

---

### User Story 2 - Comprehensive Master Linking for All Duplicate Copies (Priority: P1)

As an archivist ingesting a folder that contains multiple copies of the exact same content under different filenames (e.g., 5 copies of the same video or photo), I want the system to designate exactly one file as the authoritative canonical master on the primary storage and automatically link all remaining copies in the database to this master, marking them for deletion, so that no redundant files are left unreferenced or unpruned.

**Why this priority**: When users have clusters of identical files, ingesting one copy must resolve the entire cluster. Leaving sibling copies without master references causes pruning to fail or skip them as unverified duplicates.

**Independent Test**: Create a test folder with 5 identical copies of a file with distinct names. Run the 3-step workflow. Verify that exactly 1 file is transferred to the primary master archive, 1 canonical master record is added in the database, and all other 4 duplicate records in the database are marked for deletion with direct references to the canonical master ID and primary path.

**Acceptance Scenarios**:

1. **Given** a content group of 5 identical files across one or more source folders, **When** the deduplication evaluation executes, **Then** exactly one copy is selected for master ingestion and the other 4 copies are flagged for removal.
2. **Given** the selected copy is ingested into the primary master drive, **When** the ingest transaction completes, **Then** the primary master entry is stored in the database, and all other 4 duplicate copies are updated to link their master reference to this new canonical master.
3. **Given** duplicate files marked for removal with valid master references, **When** the operator runs duplicate pruning, **Then** all redundant copies are verified against the canonical master and safely deleted without warnings or unverified master skips.

---

### User Story 3 - Replica Synchronization State Tracking (Priority: P2)

As a backup manager configuring one or more secondary replica backup drives, I want every canonical master asset in the database to track its replication lifecycle status (such as pending replication, partially replicated, or fully replicated), so that the system and operator always know which master assets are backed up and which are awaiting synchronization.

**Why this priority**: Master archives without verified replicas risk permanent data loss upon hardware failure. Maintaining explicit replication state on the master record provides immediate visibility into backup completeness.

**Independent Test**: Ingest a new media file with two replica drives defined in the configuration. Verify that upon master ingestion, the master record's replication status is set to pending replication across both replica drives. After running replica synchronization to the first drive, verify status updates to partially replicated, and after syncing to the second drive, verify it transitions to fully replicated.

**Acceptance Scenarios**:

1. **Given** a newly ingested canonical master asset and one or more configured replica targets, **When** the master record is registered in the database, **Then** its replication state is initialized to indicate that replica copies are pending.
2. **Given** an asset with pending replica synchronization, **When** a replica drive is connected and synchronized, **Then** the database records the successful replication to that target and updates the overall replication status accordingly.
3. **Given** an asset that has been verified on all configured replica drives, **When** catalog status is queried, **Then** the asset is marked as fully replicated.

---

### User Story 4 - Seamless 3-Step CLI Workflow & Status Feedback (Priority: P3)

As a command-line operator running the sequence of discovery, deduplication, and ingestion commands, I want each command to emit clear, structured progress and summary feedback detailing how many assets were indexed, how many duplicates were grouped, how many masters were added to the primary drive, and how many are pending replication, so that I have complete confidence in the archive state at each step.

**Why this priority**: Operator trust requires visibility. Clear console summaries prevent confusion about what was copied, what was saved in the database, and what remains pending.

**Independent Test**: Run the 3 commands sequentially on a sample directory and observe stdout/stderr to verify that each stage reports exact counts: scanned items, deduplication candidate tallies, transferred master files, database records committed, and pending replica counts.

**Acceptance Scenarios**:

1. **Given** the initial scan command, **When** execution finishes, **Then** the summary displays total files discovered, metadata extracted, fingerprints calculated, and records saved in the database.
2. **Given** the deduplication command, **When** execution finishes, **Then** the summary displays total duplicate clusters found, items marked for ingestion to master, and items marked for removal.
3. **Given** the ingest command, **When** execution completes, **Then** the report details files copied to master, master records added to the database, duplicate records linked, and total assets pending replication.

---

### Edge Cases

- **Identical Asset Already Present in Primary Master**: What happens if an ingested file is already present on the primary master drive in the correct canonical location with identical content?
  - *Resolution*: The system skips redundant file transfer, verifies the existing file, ensures the canonical master database record exists with full metadata, links all duplicate source records to this master, and flags source copies for removal.
- **Multiple Identical Files Discovered in Separate Ingest Batches**: What happens if a duplicate file is ingested today and another copy of the same file is discovered next month?
  - *Resolution*: The newly discovered file is matched by cryptographic hash during deduplication, recognized as a duplicate of the existing canonical master, immediately linked to the existing master ID and path, and marked for removal without creating a second master file.
- **Offline Replica Drives During Ingest**: What happens if replica drives are disconnected when the ingest command runs?
  - *Resolution*: The master asset is successfully ingested into the primary drive and registered in the database with status indicating replication is pending for the offline targets. Ingestion succeeds without error, and replication occurs when the replica drives are attached.
- **Target Filename Collision with Different Content**: What happens if a file with the same name but different content hash already exists in the destination date folder on the primary drive?
  - *Resolution*: A deterministic non-colliding numeric suffix (e.g., `_1`, `_2`) is appended before the file extension, ensuring both unique files are stored and indexed distinctly without data loss.

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: During ingestion, the system MUST create or update an authoritative database record representing the file stored at the primary master destination path.
- **FR-002**: The primary master database record MUST have its deduplication status set to canonical master (`CanonicalMaster`).
- **FR-003**: The primary master database record MUST contain complete file context (file path on primary volume, file name, byte size, category, creation date) and content fingerprint (cryptographic hash).
- **FR-004**: When ingesting any asset belonging to a duplicate content cluster, the system MUST update ALL non-master records sharing the same cryptographic hash in the database to link their master identifier (`canonical_master_id`) and master path (`canonical_master_path`) to the newly established canonical master.
- **FR-005**: All duplicate source records linked to the canonical master MUST have their deduplication status set to marked for removal (`MarkForRemoval`) unless they reside in a designated replica archive structure.
- **FR-006**: The system MUST maintain a replication tracking state on each canonical master asset, indicating whether replication to configured backup drives is pending, partially complete, or fully replicated.
- **FR-007**: When replica synchronization executes, the system MUST update the replication tracking state on the canonical master record to reflect verified copies on replica volumes.
- **FR-008**: The ingestion execution report MUST explicitly report the count of new canonical master records added to the database and the count of duplicate records linked.
- **FR-009**: The deduplication and pruning commands MUST be capable of verifying canonical masters by both explicit identifier linkage and bitwise cryptographic hash matching.

---

### Key Entities *(include if feature involves data)*

- **Canonical Master Record**: A first-class database document representing a verified media file stored in the structured primary master archive (`<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}`). It includes primary filesystem path, file size, media category, capture timestamp, cryptographic content hash, and replication lifecycle status.
- **Redundant Duplicate Record**: A database document representing a copy of an asset located on a source, staging, or secondary drive. It includes a reference to the canonical master document ID and canonical master filesystem path, with status `MarkForRemoval`.
- **Master Replication State**: Structured metadata associated with each canonical master record that tracks backup status across configured replica drives, including pending replica targets, completed replica targets with synchronization timestamps, and overall replication status (`Pending`, `Partial`, `Complete`).

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of media files successfully ingested to the primary master drive have a corresponding database record with status `CanonicalMaster` and exact primary file path.
- **SC-002**: In 100% of multi-file duplicate clusters ingested, all non-master duplicate records in the database are updated with valid `canonical_master_id` and `canonical_master_path` values pointing to the master record.
- **SC-003**: Subsequent duplicate pruning operations delete 100% of redundant duplicate files without encountering "unverified master" skips or errors.
- **SC-004**: Replication status is queryable for all canonical masters, accurately identifying 100% of assets pending synchronization to configured replica drives.
- **SC-005**: Running the 3-step sequence (`scan` -> `dedup` -> `ingest`) on a folder of identical copies yields exactly 1 master file on the primary drive, exactly 1 canonical master record in the database, and N-1 records marked for removal linked to that master.

---

## Assumptions

- The primary master drive is mounted and its master root directory is accessible when running `ingest`.
- Deduplication grouping relies on bitwise cryptographic content hashing (BLAKE3) to guarantee that files with identical hashes are exact duplicates regardless of file naming differences.
- Replica drive configurations are defined in the drives configuration file or environment variables, identifying which drive groups serve as backup replicas.
- The 3-step command sequence is the standard archival workflow for ingesting new external media into the central library.
