# Feature Specification: Unified Master Ingestion and Multi-Replica Synchronization

**Feature Branch**: `012-unified-ingest-replication`

**Created**: 2026-11-10

**Status**: Draft

**Input**: User description: "we have built the dedup way to mark the duplicates, and copy to primary and replicas. what is the files in not a duplicate, still i want to copy the file into primary and replicas. how to handle it? handle the if any redundant logic or structure"

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Universal Ingest Classification for Unique and Duplicate Media (Priority: P1) 🎯 MVP

As a media curator who scans external hard drives and camera memory cards into a global catalog, I want the system to evaluate all media assets—both unique files and duplicate files—against my configured primary master drive, so that any unmastered asset is automatically marked for ingestion into the primary master archive regardless of whether it has duplicate copies.

**Why this priority**: Previously, only duplicate clusters were evaluated. Unique assets discovered on external drives or staging folders were completely overlooked. Every unique family photo or video must be identified and queued for archiving into the primary master collection.

**Independent Test**: Can be tested by indexing a mix of 5 unique photos and 5 duplicate photos residing on an external staging drive. Running catalog classification flags all 5 unique photos and exactly 1 canonical copy of each duplicate cluster as `NeedsIngestToMaster`, while marking the remaining redundant duplicate copies for removal.

**Acceptance Scenarios**:

1. **Given** media assets indexed in the catalog that reside outside the Primary Master hierarchy and have no duplicate copies, **When** catalog classification executes, **Then** each unique asset is classified as `NeedsIngestToMaster`.
2. **Given** a cluster of duplicate media assets where no copy currently exists in the Primary Master hierarchy, **When** catalog classification executes, **Then** the best-ranked copy is designated as `NeedsIngestToMaster` and all other copies are designated as `MarkForRemoval`.
3. **Given** a media asset already residing inside the Primary Master hierarchy in the correct date-and-category folder structure, **When** catalog classification executes, **Then** that asset is confirmed as `CanonicalMaster` and never flagged for ingestion.

---

### User Story 2 - Automated Bitwise-Verified Ingestion to Primary Master (Priority: P1)

As an archivist with my primary master drive and a source drive connected, I want the system to transfer all assets marked `NeedsIngestToMaster` into the primary master drive's structured date-and-category hierarchy, verify their bitwise integrity, register the newly written files as `CanonicalMaster`, and mark the source copies for removal, so that all unique media is safely secured into the master repository.

**Why this priority**: Ensures zero data loss when moving assets from temporary or secondary drives to the permanent master storage.

**Independent Test**: Connect a source volume containing 10 unmastered media files and a primary master volume. Run the master ingestion process. Verify that all 10 files are placed in their proper `<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}` folders on the primary master volume with identical byte sizes and content hashes, their catalog records reflect `CanonicalMaster`, and the source files are flagged for removal.

**Acceptance Scenarios**:

1. **Given** assets marked `NeedsIngestToMaster` on a connected source drive and an accessible primary master volume, **When** master ingestion is executed, **Then** the files are copied into the primary master volume under `<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}`.
2. **Given** an ingested file written to the primary master volume, **When** the file transfer completes, **Then** the system verifies that the byte size and cryptographic digest of the copied file match the source file before creating a `CanonicalMaster` catalog record.
3. **Given** a successful master ingestion and verification, **When** catalog records are updated, **Then** the source asset's status transitions to `MarkForRemoval` with a direct reference to the newly created master asset.
4. **Given** a file transfer failure, write error, or hash mismatch, **When** integrity verification fails, **Then** the destination file is purged, the source asset remains in `NeedsIngestToMaster`, and an error report is recorded.

---

### User Story 3 - Multi-Replica Synchronization & Fanout (Priority: P1)

As a backup administrator maintaining secondary and tertiary backup drives (`replica1`, `replica2`), I want the system to replicate all `CanonicalMaster` files from the primary master drive to all connected replica drives in an identical folder hierarchy, so that my backup drives remain exact mirrors of the master archive.

**Why this priority**: A master archive without verified replica backups is vulnerable to hardware failure. Replicas must mirror the primary master's structure and contents across all configured backup disks.

**Independent Test**: Configure a primary master drive and two replica drives (`replica1`, `replica2`). Run replica synchronization. Verify that files present in the primary master but missing from `replica1` or `replica2` are copied to the respective replica drives at matching relative paths, bitwise verified, and registered as valid replicas (`ReplicaKeep`).

**Acceptance Scenarios**:

1. **Given** a `CanonicalMaster` asset on the primary master drive and one or more mounted replica drives missing this asset, **When** replica synchronization runs, **Then** the asset is copied to each missing replica drive preserving the exact canonical subpath (`<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}/<Filename>`).
2. **Given** a replica copy written to a replica drive, **When** the copy finishes, **Then** its byte size and cryptographic digest are verified against the master before the replica record is registered in the catalog as `ReplicaKeep`.
3. **Given** multiple replica drives where one replica drive is offline/unmounted and another is connected, **When** replica synchronization runs, **Then** the connected replica drive is synchronized successfully, while pending replication for the offline drive is tracked so it can resume once the drive is attached.

---

### User Story 4 - Unified Architecture & Redundancy Elimination (Priority: P2)

As a system maintainer, I want deduplication, master ingestion, and replica synchronization to use a single unified asset lifecycle engine and shared file transfer/verification pipeline, so that there is no redundant code or conflicting state logic between duplicate handling and unique asset handling.

**Why this priority**: Eliminates bugs caused by divergent code paths. Both unique files and duplicate files follow the exact same archival contract: exactly 1 Canonical Master on Primary, 1 Replica copy per configured replica drive, and 0 redundant copies elsewhere.

**Independent Test**: Inspect the lifecycle state transitions and execution logs during a mixed run containing both duplicate clusters and unique unmastered assets; verify that both asset types pass through the exact same classification rules, transfer pipelines, and verification routines.

**Acceptance Scenarios**:

1. **Given** an asset classification run, **When** evaluating any asset in the catalog, **Then** the system applies a single unified evaluation rule: if no copy is in primary master, select best candidate for `NeedsIngestToMaster`; mark redundant copies for removal; evaluate missing replica copies for all canonical masters.
2. **Given** file transfer operations for both primary master ingestion and replica fanout, **When** files are copied, **Then** both workflows use the same verified streaming transfer component, error handling, and progress telemetry.

---

### Edge Cases

- **Target Filename Collision**: When ingesting a file into the primary master or replica hierarchy, what happens if a file with the same name already exists in the destination folder?
  - *Resolution*: If the existing destination file has the exact same cryptographic hash, it is recognized as already present. If it has a different hash, a deterministic non-colliding suffix (e.g., `_1`, `_2`) is appended before file extension, preserving both distinct assets without data corruption.
- **Missing or Corrupted Date Metadata**: What happens when an unmastered media file lacks EXIF/video capture dates or contains corrupted timestamps (e.g., year 1904 or year 2070)?
  - *Resolution*: Fallback to filesystem last modified timestamp if plausible; if completely missing or invalid, route to an organized fallback directory (`UnknownDate/<MediaCategory>`) within the master hierarchy so the asset is never dropped.
- **Partially Mounted Replica Drives**: What happens if `replica1` is mounted but `replica2` is disconnected during replica sync?
  - *Resolution*: The system synchronizes all available connected replica drives, logs a clear warning about offline replica drives, and records pending replication status so synchronization resumes seamlessly once the missing drive is reconnected.
- **Interrupted File Transfer**: What happens if power or disk connection is lost mid-copy?
  - *Resolution*: Partial target files are detected via hash and size verification failure, purged immediately or on the next run, and the database status remains unchanged until a complete, verified transfer succeeds.
- **Asset Unique to a Replica Drive**: What happens if a unique file exists on a replica drive but not on the primary master drive?
  - *Resolution*: Because it is missing from the primary master, it is classified as `NeedsIngestToMaster`. Once copied to the primary master, the original file on the replica drive transitions to `ReplicaKeep` rather than being marked for removal.

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST perform catalog-wide classification covering both unique media assets and duplicate media assets in a single unified evaluation pass.
- **FR-002**: System MUST identify every media asset lacking a verified copy in the configured Primary Master directory hierarchy and designate it as `NeedsIngestToMaster`.
- **FR-003**: System MUST designate redundant copies of an asset that exist outside the Primary Master and Replica Master hierarchies as `MarkForRemoval`.
- **FR-004**: System MUST support ingesting assets marked `NeedsIngestToMaster` into the Primary Master hierarchy using the standard date and media category folder structure (`<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}`).
- **FR-005**: System MUST verify the bitwise integrity (byte size and cryptographic content hash) of each copied file at the destination before updating catalog status records.
- **FR-006**: System MUST update the catalog record of an ingested file on the Primary Master to `CanonicalMaster` upon successful bitwise verification.
- **FR-007**: System MUST transition source asset records to `MarkForRemoval` referencing the new `canonical_master_id` only after the primary master copy has been verified and registered.
- **FR-008**: System MUST support synchronizing verified `CanonicalMaster` assets from the Primary Master drive to all configured and mounted replica drives (`replica1`, `replica2`, etc.).
- **FR-009**: System MUST replicate files to replica drives using the exact relative master hierarchy path (`<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}/<Filename>`).
- **FR-010**: System MUST register verified replica copies in the catalog as `ReplicaKeep`, referencing their associated `canonical_master_id`.
- **FR-011**: System MUST track replication status per configured replica drive, identifying which replica drives have confirmed copies and which have pending copies.
- **FR-012**: System MUST provide preview / dry-run options for both classification, master ingestion, and replica synchronization without altering disk files or database states.
- **FR-013**: System MUST eliminate redundant code logic by sharing file transfer, verification, collision avoidance, and path generation modules across both master ingestion and replica synchronization workflows.
- **FR-014**: System MUST emit live progress telemetry (items completed, data volume transferred, transfer rates, elapsed time) during ingestion and replication operations.

---

### Key Entities *(include if feature involves data)*

- **MediaAssetCatalogRecord**: Represents the indexed metadata of a media file, including path, drive association, file size, capture date, media category, cryptographic hash, and deduplication/lifecycle status.
- **AssetLifecycleStatus**: The operational state of a catalog record:
  - `CanonicalMaster`: The authoritative master copy located on the primary master volume.
  - `ReplicaKeep`: A verified secondary or tertiary mirror copy located on a designated replica volume.
  - `NeedsIngestToMaster`: An unmastered asset (unique or duplicate candidate) located on a source or staging volume that must be transferred to the primary master volume.
  - `MarkForRemoval`: A redundant copy located outside the canonical master or replica paths, eligible for safe pruning.
  - `ConflictReview`: An asset requiring manual operator resolution due to irreconcilable metadata or path discrepancies.
- **ReplicaSyncStatus**: Tracks the replication presence of a canonical master across configured replica drives (e.g., list of replica drive identifiers where the asset is confirmed vs. pending).
- **DriveRole**: Classification of storage volumes into `primary`, `replica1`, `replica2` (and subsequent replicas), or `staging`/`external`.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of unique media assets indexed in the catalog that reside outside the Primary Master hierarchy are detected and scheduled for master ingestion.
- **SC-002**: Zero data loss: 100% of copied assets undergo bitwise cryptographic hash and size verification before any source record is marked for removal.
- **SC-003**: 100% of files ingested into the Primary Master and replica drives conform to the standard date-and-category folder structure (`<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}`).
- **SC-004**: Replication can fan out canonical masters to N configured replica drives without manual per-drive scripts or manual file management.
- **SC-005**: All duplicate handling and unique asset handling operate through a single unified catalog classification pipeline, eliminating redundant duplicate-only evaluation routines.
- **SC-006**: Ingestion and replication workflows provide real-time transfer progress and timing telemetry throughout batch operations.

---

## Assumptions

- **Drive Configuration**: Primary and replica drive definitions, mount paths, and master directory roots are loaded from the existing environment (`.env`) or configuration file (`drives.yaml`).
- **Cryptographic Hashes**: All assets have their cryptographic hashes (e.g. Blake3) pre-computed during indexing; classification does not require on-the-fly re-hashing of source disks unless verifying newly written files.
- **Replication Trigger**: Master ingestion and replica synchronization can be invoked either as sequential stages of a unified command (e.g., `ingest --replicate`) or as distinct workflow commands (`ingest` and `sync-replicas`).
- **Destructive Deletion**: Physical file deletion remains an explicit, opt-in pruning step (`prune`), ensuring that marking a file as `MarkForRemoval` never deletes files prematurely without explicit operator invocation.
- **Single Master Authority**: Each unique asset has exactly one `CanonicalMaster` on the designated primary drive. Backup drives mirror this master as `ReplicaKeep`.
