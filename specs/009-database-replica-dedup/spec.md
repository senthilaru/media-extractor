# Feature Specification: Database-Driven Replica-Aware Media Deduplication

**Feature Branch**: `009-database-replica-dedup`

**Created**: 2026-11-09

**Status**: Draft

**Input**: User description: "i want to change the logic of dedup & dedup-stream, it has to read through the unified metadata maintainted in mongodb, and mark them as duplicate. Now, in order to remove the duplicate files, i will maintain few group of external drives as primary and another group as replica1, later i may add another replica. All this drive info will be in the configuration file. There is a main path in the primary & replicas like \"/Volume/SP-1/SPSA/media/master\", this path maintains neat folder structure like \"2021/30-Nov-2021/Videos\", \"2021/30-Nov-2021/Photos\", \"2021/30-Nov-2021/PhotosRaw\""

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Database-Driven Duplicate Identification & Marking (Priority: P1) 🎯 MVP

As a media archivist with 10+ external hard drives indexed into a centralized database, I want `dedup` and `dedup-stream` to read asset records directly from the database, identify duplicate media files using pre-computed cryptographic and perceptual content fingerprints, and record their duplicate status and canonical relationships directly in the database records, so that I do not need all 10 hard drives mounted at the same time to perform global duplicate detection.

**Why this priority**: Solves the core problem where individual drives cannot be held in memory or mounted concurrently. The database contains the complete global index of all assets across all drives, enabling immediate, offline-capable duplicate clustering.

**Independent Test**: Can be tested by pre-populating the database with metadata records representing assets across multiple drives (both connected and disconnected), running `metadata-extractor dedup`, and verifying that duplicate clusters are formed from database records and each document is updated with appropriate duplicate classification fields without requiring any local filesystem access.

**Acceptance Scenarios**:

1. **Given** a database populated with media metadata from multiple drives, **When** the operator runs `metadata-extractor dedup`, **Then** the engine reads and streams records from the database instead of scanning local directories from disk.
2. **Given** two or more database records sharing identical cryptographic content hashes (`fingerprint.cryptographic_hash`), **When** duplicate evaluation executes, **Then** the engine clusters them as exact bitwise duplicates.
3. **Given** clustered duplicate records in the database, **When** deduplication evaluation completes, **Then** each document in the database is updated with deduplication metadata (including `dedup.status`, `dedup.cluster_id`, and `dedup.canonical_master_id`).
4. **Given** a streaming pipeline `metadata-extractor dedup-stream`, **When** metadata documents are piped from database queries via stdin, **Then** the stream emits updated deduplication clusters and action records in newline-delimited JSON.

---

### User Story 2 - Drive Group Hierarchy & Canonical Master Structure (Priority: P1)

As a home media server administrator, I want the deduplication engine to recognize drive roles (`primary`, `replica1`, `replica2`, etc.) and canonical master directory patterns (`<MasterRoot>/<Year>/<DD-Mon-YYYY>/<CategoryFolder>`), so that files organized inside the primary master hierarchy are automatically designated as the canonical masters, intended backup copies in replica master folders are preserved as valid replicas, and all redundant or unorganized copies scattered across other folders and drives are marked for removal.

**Why this priority**: Prevents destructive accidental deletion by establishing an unambiguous, rule-based hierarchy. Master assets in the well-organized path must never be marked for deletion; only redundant copies scattered outside the canonical master path should be flagged for removal.

**Independent Test**: Can be tested with a dataset where the same photo exists in:
1) `/Volume/SP-1/SPSA/media/master/2021/30-Nov-2021/Photos/IMG_1042.JPG` (Primary Master)
2) `/Volume/SP-2/SPSA/media/master/2021/30-Nov-2021/Photos/IMG_1042.JPG` (Replica1 Copy)
3) `/Volume/SP-1/Downloads/OldBackup/IMG_1042.JPG` (Scattered redundant copy on Primary)
4) `/Volume/SP-3/DCIM/IMG_1042.JPG` (Unorganized copy on another disk)
Verify that copy (1) is marked as `Master`, copy (2) is marked as `ReplicaKeep`, and copies (3) and (4) are marked as `MarkForRemoval`.

**Acceptance Scenarios**:

1. **Given** duplicate files across drives, **When** evaluating candidate precedence, **Then** the system assigns highest priority to files located within the configured master directory path on `primary` drives matching the pattern `<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}`.
2. **Given** a duplicate file located in the canonical master path on a `replica` drive matching an existing primary master, **When** duplicate marking executes, **Then** the system tags the replica file as `ReplicaKeep` rather than marking it for deletion.
3. **Given** duplicate files located outside the canonical master paths (e.g. scratch directories, downloads, camera dumps), **When** a primary master already exists, **Then** the system tags the redundant copies as `MarkForRemoval` with reference to the canonical master path.
4. **Given** a unique file that exists only in an unorganized folder and does NOT exist in any master path, **When** deduplication runs, **Then** the system tags the file as `NeedsIngestToMaster` rather than marking it for removal.

---

### User Story 3 - External Drive Configuration File & Group Role Mapping (Priority: P2)

As an operator with diverse hard drives, I want to define drive identifiers, mount points, group roles (`primary`, `replica1`, `replica2`), and master root paths in a clear configuration file (e.g. `drives.yaml` or `drives.toml`), so that I can easily add or reassign hard drives over time without modifying code or hardcoding drive paths.

**Why this priority**: Drive configurations change as external drives are plugged, unplugged, replaced, or expanded with new replica tiers. A declarative configuration file makes drive group management transparent and repeatable.

**Independent Test**: Can be tested by providing a sample `drives.yaml` configuring multiple drives with different roles and verifying that the deduplication engine parses the configuration, maps file paths to their respective drive groups, and rejects invalid or conflicting configurations with descriptive validation errors.

**Acceptance Scenarios**:

1. **Given** a configuration file defining `primary` and `replica` drive groups with root paths, **When** the operator invokes `metadata-extractor dedup --config drives.yaml`, **Then** the engine successfully loads and applies the drive role hierarchy.
2. **Given** a configuration file with missing or conflicting drive paths, **When** validation executes, **Then** the system fails fast with a clear error indicating the misconfigured drive entry.
3. **Given** a command invoked with `--dry-run`, **When** deduplication evaluates the database, **Then** it displays the planned classification counts per drive and role without writing changes to the database.

---

### User Story 4 - Per-Drive Removal Manifest & Safe Deletion Execution (Priority: P2)

As an operator cleaning up 10 external hard drives one at a time, I want the system to generate a per-drive duplicate removal manifest (and provide a drive-specific prune command), so that I can plug in one hard drive at a time, review the marked duplicates for that specific drive, and safely delete only the redundant copies without touching master or replica files.

**Why this priority**: The operator does not have all 10 drives connected at once. Generating targeted, drive-by-drive removal lists allows safe, incremental physical disk cleanup whenever a specific drive is mounted.

**Independent Test**: Can be tested by generating a removal manifest for `Drive-3`, connecting a drive matching that root path, running the cleanup command in interactive review mode, and verifying that only the files marked `MarkForRemoval` on that drive are deleted while verifying that primary masters remain untouched.

**Acceptance Scenarios**:

1. **Given** duplicate marking completed in the database, **When** the operator requests a removal summary, **Then** the system reports reclaimable storage grouped by drive and group role.
2. **Given** a specific drive mounted (e.g., `/Volumes/Drive-3`), **When** the operator runs the cleanup tool targeting that drive, **Then** it reads only the database records marked `MarkForRemoval` for that specific drive's mount path.
3. **Given** a file marked `MarkForRemoval`, **When** deletion is executed, **Then** the system verifies the canonical master still exists before deleting the redundant file, preventing accidental data loss if the master drive is corrupt or missing.
4. **Given** successful deletion of redundant files on a drive, **When** cleanup completes, **Then** the database records for the deleted paths are updated with `dedup.status = "Removed"` and the reclaimed byte volume is logged.

---

### Edge Cases

- **Master File Missing on Primary Drive**: If an unorganized copy is found on Drive-B, but the presumed master record on Drive-A has not been verified or is missing from disk, the system MUST NOT delete the unorganized copy; it must flag it as `NeedsVerification` to prevent permanent data loss.
- **Drive Mounted at a Different Path**: If an external disk mounts as `/Volumes/SP-1 1` due to an OS mount collision, the drive configuration MUST support volume UUIDs or disk labels alongside path prefixes to resolve the correct drive role.
- **Multiple Copies Inside Master Hierarchy**: If two identical files exist within the master directory structure under different dates or names, the system MUST flag this as a `MasterConflict` for manual operator review rather than automatically deleting either copy.
- **Format / Transcode Variants Across Drives**: When an original RAW/Photo exists in Master and a resized thumbnail or recompressed variant exists on a replica, the replica MUST retain the master format unless the replica is explicitly configured as a compressed mirror.
- **Non-Standard Date Formats in Legacy Paths**: If existing folders have names like `2021-11-30` instead of `30-Nov-2021`, the path parser MUST handle both canonical and legacy date patterns when evaluating master folder conformity.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The `dedup` command MUST query metadata documents directly from the configured MongoDB database rather than requiring local directory scanning.
- **FR-002**: The system MUST support a declarative configuration file (e.g., `drives.yaml` or `drives.toml`) that defines drive groups (`primary`, `replica1`, `replica2`, etc.), their mount paths or volume identifiers, and their canonical master sub-paths.
- **FR-003**: The system MUST recognize the canonical master path hierarchy: `<MasterRoot>/<Year>/<DD-Mon-YYYY>/<CategoryFolder>` where `<CategoryFolder>` is `Videos`, `Photos`, or `PhotosRaw`.
- **FR-004**: When duplicates are detected across records, the system MUST designate the copy located within the `primary` group's canonical master path as the `CanonicalMaster`.
- **FR-005**: If an identical asset exists within the canonical master path of a `replica` drive, the system MUST mark that record as `ReplicaKeep` and link it to the primary master.
- **FR-006**: Duplicate copies found outside the canonical master path on either primary or replica drives MUST be marked as `MarkForRemoval`.
- **FR-007**: Any media asset that exists ONLY outside the master path (with zero copies in any master path) MUST be marked as `NeedsIngestToMaster` and MUST NOT be marked for removal.
- **FR-008**: The system MUST record deduplication evaluation results directly into the database document under a structured `dedup` field (storing `status`, `cluster_id`, `canonical_master_id`, `master_file_path`, `drive_group`, and `evaluated_at`).
- **FR-009**: The system MUST provide an execution mode to generate per-drive removal manifests detailing all files marked for removal, their byte sizes, and their corresponding canonical master locations.
- **FR-010**: The system MUST support an incremental, drive-by-drive deletion workflow that allows the operator to connect one drive at a time and prune only the marked duplicate files for that mounted drive.
- **FR-011**: Before deleting any file marked for removal, the deletion workflow MUST verify that the designated canonical master record exists in the database and is confirmed intact.
- **FR-012**: The `dedup-stream` command MUST accept metadata documents from stdin (e.g., exported from MongoDB queries) and emit updated duplicate classification records in streaming NDJSON.

### Key Entities *(include if feature involves data)*

- **DriveConfiguration**: Represents the mapping of physical storage drives to logical roles:
  - `drive_id`: Unique identifier (e.g. `primary_drive_1`, `backup_seagate_4tb`).
  - `role`: Role tier (`Primary`, `Replica1`, `Replica2`, etc.).
  - `mount_path`: File system mount point (e.g. `/Volumes/SP-1`).
  - `master_relative_path`: Path from mount point to master root (e.g. `SPSA/media/master`).
- **DeduplicationStatus**: Enum representing an asset's evaluated role in the archive:
  - `CanonicalMaster`: The authoritative master file in the primary master directory structure.
  - `ReplicaKeep`: An approved mirror copy inside an authorized replica master directory structure.
  - `MarkForRemoval`: A redundant copy located outside canonical master paths scheduled for deletion.
  - `NeedsIngestToMaster`: A unique asset found only in an unorganized folder that must be copied into the master hierarchy before cleanup.
  - `ConflictReview`: Multiple conflicting copies exist within master directories requiring operator intervention.
  - `Removed`: A previously marked duplicate that has been physically deleted from disk.
- **DeduplicationMetadata**: Sub-document embedded within `MediaMetadataDocument` in MongoDB:
  - `cluster_id`: UUID grouping identical assets.
  - `status`: `DeduplicationStatus`.
  - `canonical_master_id`: Document UUID of the canonical master.
  - `canonical_master_path`: Full path to the master asset on the primary drive.
  - `drive_group`: The drive role name assigned to this copy (`Primary`, `Replica1`, etc.).
  - `is_in_master_path`: Boolean flag indicating whether the file conforms to the canonical folder structure.
  - `reclaimable_bytes`: Byte size that will be freed if this copy is deleted.
- **DriveRemovalManifest**: Report and action list generated for an individual drive:
  - `drive_id` and `mount_path`.
  - `total_files_to_remove` and `total_reclaimable_bytes`.
  - List of file removal tasks pairing each target path with its confirmed canonical master path.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Global deduplication evaluation over 50,000+ database records executes in under 10 seconds without mounting the physical drives.
- **SC-002**: 100% of files situated in the canonical master directory structure on primary drives are preserved as `CanonicalMaster` and are never marked for removal.
- **SC-003**: 100% of duplicate files in replica groups located in the matching master hierarchy are marked as `ReplicaKeep` rather than deletion candidates.
- **SC-004**: Zero unique media files are marked for removal; any asset lacking a canonical master is strictly marked as `NeedsIngestToMaster`.
- **SC-005**: The per-drive cleanup tool allows an operator to safely reclaim storage on any single mounted drive in under 1 minute per 1,000 files, verifying master existence before every deletion.

## Assumptions

- Metadata extraction has already scanned the media drives into the unified MongoDB collection using `metadata-extractor <PATH> --mongodb-uri ...`.
- External drives mount under standard operating system paths (e.g., `/Volumes/<DriveName>` on macOS or `/mnt/<DriveName>` on Linux).
- The canonical master folder structure adheres to `<MasterRoot>/<Year>/<DD-Mon-YYYY>/<CategoryFolder>` where category folder names are `Videos`, `Photos`, or `PhotosRaw`.
- Deletions are intentional and performed by the operator on mounted drives using the generated drive manifests; the system defaults to safe simulation (`--dry-run`) unless explicit confirmation is provided.
