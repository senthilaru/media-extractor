# Feature Specification: Multi-Volume Storage Extension Pools for Primary Master and Replicas

**Feature Branch**: `024-storage-extension-pools`

**Created**: 2026-11-20

**Status**: Draft

**Input**: User description: "I have a primary and few replica, now both are almost full in storage capacity, i want to add another external harddisk as additional primary and replica, in order to extend the storage. we can have the folder names as same like 2011, 2026 can be found in both primary and primary extension. but, it should maintain single copy of files in it. eg: primary+primary_extension_1+primary_extension_2,etc.. replica_1+replica_1_extension_1+replica_1_extension_2,etc.. replica_2+replica_2_extension_1+replica_2_extension_2,etc.. you do better namings"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Storage Pool Definition & Single Canonical Copy Across Extension Volumes (Priority: P1) 🎯 MVP

As a media archivist whose primary master drive has run out of storage capacity, I want to attach and configure one or more additional external hard drives as primary storage extension volumes (`primary_vol1`, `primary_vol2`, `primary_vol3`), so that the system treats them as a single unified primary master archive where identical date hierarchies (e.g. `2011`, `2026`) can exist on any volume, while strictly maintaining exactly one single canonical copy of each unique media asset across all volumes in the pool.

**Why this priority**: Without storage pool extension, users with full hard drives cannot ingest new media without manually partitioning their library, risking duplicates across multiple drives or breaking centralized deduplication.

**Independent Test**: Configure a primary pool with two drives (`primary_vol1` and `primary_vol2`), each holding a `2026/01-Jan-2026` folder. Scan and run deduplication. Verify that assets on either volume are recognized as `CanonicalMaster`, and an identical file present on both `primary_vol1` and `primary_vol2` is detected as redundant with only one retained as the canonical keeper.

**Acceptance Scenarios**:

1. **Given** a primary storage pool configured with multiple member volumes (`primary_vol1`, `primary_vol2`), **When** duplicate analysis runs, **Then** an asset residing in the canonical master path of ANY volume in the primary pool is recognized as a `CanonicalMaster`.
2. **Given** the same file exists on both `primary_vol1` and `primary_vol2`, **When** duplicate analysis runs, **Then** exactly one copy is classified as `CanonicalMaster` (based on configured volume sequence), and the secondary copy is marked `MarkForRemoval` to enforce the single-copy invariant across the primary pool.
3. **Given** a file already exists on `primary_vol1`, **When** duplicate analysis evaluates a staging drive, **Then** the file on `primary_vol1` is confirmed as `CanonicalMaster`, and the staging file is marked `MarkForRemoval` without scheduling any redundant ingestion to `primary_vol2`.

---

### User Story 2 - Capacity-Aware Ingestion & Volume Spillover (Priority: P1)

As an archivist running master ingestion, when `primary_vol1` is full or reaches its safety free-space threshold, I want new unique media assets to automatically spill over and ingest into the next available volume in the pool (`primary_vol2`), creating the standard date hierarchy (`<Year>/<DD-Mon-YYYY>/{Photos,Videos,PhotosRaw}`) on the extension drive seamlessly.

**Why this priority**: Enables continuous, hands-off ingestion across multi-terabyte libraries that exceed the physical capacity of any single drive.

**Independent Test**: Set `primary_vol1` free-space safety threshold so it reports full. Ingest new unmastered media. Verify that the files are written to `primary_vol2` in the correct date folders, verified via cryptographic hashes, and cataloged in MongoDB with `primary_vol2`'s drive ID and canonical path.

**Acceptance Scenarios**:

1. **Given** `primary_vol1` has insufficient available space for an asset, **When** master ingest executes, **Then** the asset is automatically written to `primary_vol2` under `<Year>/<DD-Mon-YYYY>/<Category>/`.
2. **Given** an operator explicitly passes `--target-drive primary_vol2`, **When** master ingest executes, **Then** the specified volume is used as the primary write destination regardless of `primary_vol1` free space.
3. **Given** an asset is ingested into `primary_vol2`, **When** its database document is recorded, **Then** `file_context.scanned_root`, `file_context.file_path`, and `dedup.canonical_master_path` correctly reference `primary_vol2`.

---

### User Story 3 - Multi-Volume Replica Synchronization (Priority: P2)

As an archivist with multiple backup replica drives, when replica backup drives reach capacity, I want to add replica extension volumes (`replica_1_vol1`, `replica_1_vol2` for the first replica group, and `replica_2_vol1`, `replica_2_vol2` for the second replica group), so that `sync-replicas` and `organize` mirror the complete multi-volume primary archive across multi-volume replica sets with 100% data parity.

**Why this priority**: Backup sets must scale alongside primary storage; if replicas cannot expand across multiple volumes, the 3-2-1 backup strategy fails as soon as primary data exceeds a single disk's capacity.

**Independent Test**: Populate `primary_vol1` and `primary_vol2` with canonical masters. Configure `replica_1_vol1` and `replica_1_vol2`. Run replica synchronization. Verify that all canonical masters across both primary volumes are mirrored across the replica volumes without omission.

**Acceptance Scenarios**:

1. **Given** canonical masters distributed across `primary_vol1` and `primary_vol2`, **When** `sync-replicas` runs, **Then** every canonical master is mirrored to an active volume in the `replica_1` pool.
2. **Given** `replica_1_vol1` becomes full during replication, **When** subsequent files are transferred, **Then** replication automatically spills over to `replica_1_vol2`.
3. **Given** an existing file is already replicated on `replica_1_vol1`, **When** replication runs, **Then** the file is detected as `ReplicaKeep` and skipped without duplicating it onto `replica_1_vol2`.

---

### User Story 4 - Comprehensive Archive Protection Across All Pool Volumes (Priority: P2)

As a security-minded user, I want the system's `ArchiveProtectionGuard` to automatically detect and protect every member volume across all configured pools (all primary volumes and all replica volumes), so that programmatic deletions or pruning operations NEVER delete canonical master or replica files on any disk in any pool.

**Why this priority**: Expanding storage across more physical disks increases the blast radius of accidental deletions; all pool volumes must be strictly ring-fenced as protected archives.

**Independent Test**: Attempt to run `dedup prune` with `--drive` set to `primary_vol2` or `replica_1_vol2`. Verify that the operation fails fast with `PruneForbiddenOnProtectedArchive`.

**Acceptance Scenarios**:

1. **Given** a drive configuration with `primary_vol1`, `primary_vol2`, `replica_1_vol1`, `replica_1_vol2`, **When** `ArchiveProtectionGuard` validates paths, **Then** paths under any master directory of any pool volume are recognized as protected archive storage.
2. **Given** a prune command targeting `primary_vol2`, **When** prune validation runs, **Then** execution is aborted immediately before any disk modification occurs.

---

### Edge Cases

- **Partial Storage Pool Connected**: What if `primary_vol1` is offline but `primary_vol2` is mounted? The system must detect missing volumes, warn the user, permit read operations on connected volumes, but halt operations that require global deduplication consistency across unmounted primary volumes.
- **Identical Subfolder Across Multiple Volumes**: `2026/01-Jan-2026/Photos/` may exist on both `primary_vol1` and `primary_vol2`. This is valid and expected. However, individual file names (e.g. `IMG_0001.JPG`) within that date folder MUST only exist on one of the volumes.
- **Both Volumes Full**: If all member volumes in the primary pool exceed their safety threshold, ingestion must halt gracefully with a clear error: `StoragePoolExhausted: all primary volumes are full (add primary_volX to drives.yaml to extend)`.
- **Pre-existing Duplicate on Extension Volume**: If an extension drive previously had a manual copy of an asset before being added to the configuration, deduplication must identify it, mark one as `CanonicalMaster` and the other as `MarkForRemoval`, and report it in the audit summary.
- **Collision Resolution Across Volumes**: When ingesting a new file with the same name as a file on `primary_vol1`, but destined for `primary_vol2`, collision resolution must check uniqueness across the entire pool, ensuring filenames do not collide confusingly between volumes.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST allow multiple drive definitions to share the same group identifier (e.g. `group: "primary"`, `group: "replica1"`, `group: "replica2"`) in `drives.yaml` and environment configurations.
- **FR-002**: The system MUST treat all drives in `group: "primary"` as member volumes of the unified primary master storage pool.
- **FR-003**: The deduplication engine MUST evaluate all member volumes of the primary pool with equal highest priority (priority 100), ensuring that any asset residing in any primary volume's canonical master path is eligible as a `CanonicalMaster`.
- **FR-004**: The system MUST enforce a single-copy invariant across each storage pool: if bitwise-identical assets reside on multiple volumes of the same pool, exactly one volume's copy MUST be retained as `CanonicalMaster`, and all other intra-pool copies MUST be marked `MarkForRemoval`.
- **FR-005**: The master ingestion service MUST support volume spillover: when the primary volume's available disk space falls below a configurable threshold (default: 5 GB buffer), ingestion MUST automatically route subsequent assets to the next active volume in the pool.
- **FR-006**: Ingestion MUST preserve the canonical folder naming convention (`<Year>/<DD-Mon-YYYY>/{Photos,Videos,PhotosRaw}`) across all member volumes in a pool.
- **FR-007**: The replica synchronization service MUST mirror canonical masters from any primary pool volume to the designated replica pools, supporting multi-volume replica destinations.
- **FR-008**: The `ArchiveProtectionGuard` MUST register every member volume of `primary` and all replica groups as protected archives, forbidding programmatic deletion on any volume.
- **FR-009**: The system MUST support standard naming conventions:
  - Primary pool drives: `primary_vol1` (or `primary_main`), `primary_vol2` (or `primary_ext1`), `primary_vol3` (or `primary_ext2`), etc.
  - Replica pool drives: `replica_1_vol1`, `replica_1_vol2`, `replica_2_vol1`, `replica_2_vol2`, etc.
- **FR-010**: The configuration loader MUST support declaring storage extension volumes in environment variables (e.g. `DRIVE_PRIMARY_EXT1_MOUNT`, `DRIVE_PRIMARY_EXT1_MASTER`, `DRIVE_REPLICA1_EXT1_MOUNT`, etc.).
- **FR-011**: When a base primary drive is unmounted/offline, organizing on a mounted extension drive MUST verify duplicates against primary masters recorded in the MongoDB database catalog and safely prune eligible redundant files without requiring the offline master drive to be mounted on disk.
- **FR-012**: Ingestion into a storage pool MUST only target currently mounted member volumes, creating only necessary directory paths for ingested files without mirroring directory trees from offline volumes.

### Key Entities

- **Storage Pool**: A logical group of physical storage volumes sharing a common role (`primary`, `replica1`, `replica2`) and retention priority.
- **Pool Volume (`DriveDefinition`)**: An individual physical external drive mount point and master root belonging to a storage pool.
- **Spillover Policy**: The capacity-aware rule determining when a storage pool transitions writes from one volume to the next.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users can add an arbitrary number of extension hard drives to primary and replica pools without modifying the folder structure or splitting database catalogs.
- **SC-002**: 100% of unique files across the entire primary pool are maintained as single copies with 0 redundant duplicates retained within the pool.
- **SC-003**: Ingestion automatically transitions to extension volumes when the active volume reaches capacity without manual user intervention.
- **SC-004**: 100% of canonical masters across all primary pool volumes are verified and mirrored across all active replica backup pools.
- **SC-005**: 0% of files residing on any primary or replica pool volume can be deleted by duplicate pruning commands.

## Assumptions

- Each volume in a storage pool is formatted with a standard POSIX filesystem (APFS, HFS+, ext4, exFAT) mounted at a distinct mount point (e.g. `/Volumes/SP-1`, `/Volumes/SP-3`).
- Volume filling order defaults to the declaration order in `drives.yaml` or numeric index order (`vol1` -> `vol2` -> `vol3`).
- A minimum free-space safety buffer (default: 5 GB) prevents hard drives from being filled to 100% block capacity, preserving filesystem performance and inode allocation.
- Media items can be safely distributed across volumes by date or ingestion order; a given year/date folder may span multiple volumes as long as individual files are not duplicated.
