# Feature Specification: Apple AAE Sidecar File Support

**Feature Branch**: `019-aae-sidecar-support`

**Created**: 2026-11-13

**Status**: Draft

**Input**: User description: "while dealing with images, handle the file with extension AAE also, as its iphone related place next to the actual image/video"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Automatic Ingestion of Companion AAE Sidecars Beside Images/Videos (Priority: P1) 🎯 MVP

When organizing unorganized media exported from iPhones, any Apple edit metadata files with extension `.AAE` (or `.aae`) associated with photos or videos must be recognized as companion sidecar files and automatically copied into the canonical primary master directory right next to their corresponding media file (e.g., `media/master/<Year>/<DD-Mon-YYYY>/Photos/IMG_1234.AAE` adjacent to `IMG_1234.HEIC` or `IMG_1234.JPG`).

**Why this priority**: Without `.AAE` sidecar preservation, user edits made on iOS (cropping, lighting adjustments, filters, slow-motion markers, markup) are lost or separated when importing into the archive. Placing the sidecar adjacent to the primary asset ensures standard photo libraries and tools can read the non-destructive edits alongside the original media.

**Independent Test**: Ingest an unorganized folder containing `IMG_1001.HEIC` and its companion `IMG_1001.AAE`. Verify that both `IMG_1001.HEIC` and `IMG_1001.AAE` are placed in the same destination directory in the primary master archive.

**Acceptance Scenarios**:

1. **Given** an unorganized source folder containing an image (`IMG_4500.JPG`) and its matching sidecar (`IMG_4500.AAE`), **When** the organize or ingest command runs, **Then** both the image and the `.AAE` file are placed in the exact same date-based canonical folder in the primary master archive.
2. **Given** an unorganized source folder containing a video (`IMG_4501.MOV`) and its matching sidecar (`IMG_4501.AAE`), **When** the organize or ingest command runs, **Then** both the video and the `.AAE` file are placed in the exact same date-based canonical folder in the primary master archive.
3. **Given** sidecar files with varying case extensions (`.aae`, `.AAE`, `.Aae`), **When** scanning and ingesting, **Then** they are recognized and paired with their respective media assets case-insensitively.
4. **Given** an image that has a companion sidecar, **When** calculating target subdirectories, **Then** the `.AAE` sidecar inherits the exact same folder placement category (`Photos`, `Videos`, or `PhotosRaw`) as its companion media asset.

---

### User Story 2 - Parity Replication of AAE Sidecar Files to Backup Drives (Priority: P2)

When synchronizing master media assets from the primary archive to replica drives, all companion `.AAE` sidecar files present in the canonical master archive must be replicated alongside their corresponding media assets, maintaining 100% parity across all mirror drives.

**Why this priority**: Backup replica drives must be complete mirrors of the master archive. If photos are backed up without their accompanying `.AAE` sidecar files, data loss occurs for iOS edit metadata on all backup drives.

**Independent Test**: Run replica synchronization on a master directory containing `IMG_2000.JPG` and `IMG_2000.AAE`. Verify that the replica drive receives both files in the identical relative path structure and with verified data integrity.

**Acceptance Scenarios**:

1. **Given** a primary master archive with a photo and its companion `IMG_3000.AAE`, **When** replica synchronization runs, **Then** `IMG_3000.AAE` is copied to all active replica drives in the exact same folder as the photo.
2. **Given** an `.AAE` sidecar that already exists on a replica drive with an identical hash, **When** replica synchronization runs, **Then** the transfer is safely skipped to avoid redundant I/O.
3. **Given** a transfer of an `.AAE` file to a replica drive, **When** the copy completes, **Then** cryptographic hash verification validates the copied `.AAE` content before marking replication successful.

---

### User Story 3 - Clean Pruning of Ingested AAE Sidecar Files (Priority: P3)

When pruning redundant source media after verified ingestion and replication, companion `.AAE` sidecar files that have been successfully archived in the primary master must be pruned along with their parent media files, leaving no orphan `.AAE` files lingering in the source directory.

**Why this priority**: Leaving orphan `.AAE` sidecars behind in the source folder prevents directories from becoming empty, preventing empty folder cleanup and causing clutter for users organizing thousands of media files.

**Independent Test**: Run the full organize pipeline with an unorganized source folder containing `IMG_0050.JPG` and `IMG_0050.AAE`. Verify that upon successful master ingestion and replica sync, both files are pruned and the source folder is cleanly emptied and removed.

**Acceptance Scenarios**:

1. **Given** a source folder containing `IMG_0050.JPG` and `IMG_0050.AAE` that have been successfully ingested into master, **When** pruning executes, **Then** both `IMG_0050.JPG` and `IMG_0050.AAE` are deleted from the source folder.
2. **Given** an unorganized subfolder containing only a photo and its `.AAE` sidecar, **When** both are pruned, **Then** the empty parent subfolder is pruned, leaving the source root directory clean.
3. **Given** dry-run mode is enabled, **When** pruning is evaluated, **Then** `.AAE` deletion is simulated and reported without modifying any files on disk.

---

### User Story 4 - Handling Orphan and Pre-Ingested AAE Sidecars (Priority: P4)

Handle situations where an `.AAE` sidecar file is discovered whose companion photo or video was already previously ingested into the master archive in an earlier session, or where an `.AAE` file has no companion media asset anywhere.

**Why this priority**: Users frequently import iPhone photos in batches or multiple sessions where edit sidecars might be transferred after the main photos have already been organized.

**Independent Test**: Place an `.AAE` file in the source directory whose matching image already resides in the primary master archive. Run organize and verify the `.AAE` file is routed to the existing master directory beside its image.

**Acceptance Scenarios**:

1. **Given** an `.AAE` file in the unorganized source whose matching media file already exists in the primary master, **When** organize executes, **Then** the `.AAE` file is ingested directly into that media file's existing master folder.
2. **Given** an orphan `.AAE` file with no matching media file in the source folder or in the master archive, **When** organize executes, **Then** the orphan `.AAE` file is safely retained without crashing the pipeline, and an informational warning is logged.

---

### Edge Cases

- **Name Collisions & Renaming**: If an incoming media file encounters a name collision in the master folder and is renamed (e.g., `IMG_1234_1.JPG`), its companion sidecar MUST be renamed with the exact same base name (e.g., `IMG_1234_1.AAE`) to preserve pairing.
- **Edited Version Suffixes**: iPhones sometimes name edited files `IMG_E1234.JPG` or `OIMG_1234.JPG` with sidecars named `IMG_1234.AAE` or `IMG_E1234.AAE`. The matching logic must associate these standard Apple edit naming variants with their corresponding assets.
- **Multiple Media Candidates**: If both `IMG_1001.JPG` and `IMG_1001.MOV` (e.g. a Live Photo pair) exist in the same folder alongside `IMG_1001.AAE`, the sidecar must be placed in the directory where the primary photo/video resides without creating duplicates.
- **Zero-Byte or Corrupted .AAE**: If an `.AAE` file is 0 bytes or unreadable, the error is isolated per Constitution Principle V; the companion media file still processes normally, and the corrupt sidecar is logged.
- **Read-Only / Protected Media Archives**: Under no circumstances can an `.AAE` file be deleted from a protected primary master or replica drive during pruning.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST recognize `.AAE` (case-insensitive: `.aae`, `.AAE`, `.Aae`) as a recognized sidecar file format associated with Apple iOS/macOS image and video edits.
- **FR-002**: System MUST identify the companion relationship between an `.AAE` sidecar file and its corresponding photo or video based on matching file stem (base name) and folder location.
- **FR-003**: When ingesting a media file into the canonical primary master archive, the system MUST ingest its companion `.AAE` sidecar file into the exact same folder location right next to the media file.
- **FR-004**: If a companion `.AAE` sidecar's matching media asset is renamed due to destination collision resolution, the system MUST apply the identical base name to the `.AAE` sidecar (e.g., `IMG_1001_1.AAE` paired with `IMG_1001_1.JPG`).
- **FR-005**: All transfers of `.AAE` sidecars to master and replica drives MUST use verified stream copy with in-flight cryptographic hash verification.
- **FR-006**: During replica synchronization, the system MUST synchronize companion `.AAE` sidecars present in the primary master to all active replica drives, maintaining matching paths and file names.
- **FR-007**: During source pruning, the system MUST delete source `.AAE` files only after confirming that both the primary media file and the `.AAE` file exist and are verified in the canonical master archive.
- **FR-008**: System MUST support resolving companion relationships for Live Photos (e.g., paired `.HEIC`/`.JPG` and `.MOV` sharing an `.AAE`).
- **FR-009**: If an `.AAE` file is discovered whose companion media file is already present in the master archive, the system MUST place the `.AAE` sidecar into that existing master folder.
- **FR-010**: If an orphan `.AAE` file has no companion media file in the source or master archive, the system MUST preserve the file, avoid crashing the pipeline, and emit an informational notice.
- **FR-011**: All preview modes (`--dry-run`) MUST report `.AAE` sidecar operations (discovery, planned ingestion, replication, and pruning) without modifying any files on disk.

### Key Entities

- **Media Sidecar Asset**: A supplementary file (specifically Apple `.AAE`) containing edit instructions or metadata linked directly to a primary media item (`Photo` or `Video`).
  - Key attributes: file path, base stem, sidecar extension, companion media ID/path, byte size, cryptographic hash, pairing status (Paired, Orphan, Mastered).
- **Media Asset Pair**: An association between a primary media file (`.JPG`, `.HEIC`, `.MOV`, etc.) and its optional companion sidecar (`.AAE`), ensuring synchronized movement, backup, and cleanup.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of `.AAE` sidecars matching ingested photos or videos are placed in the exact same directory alongside their companion media assets in the primary master archive.
- **SC-002**: 100% of companion `.AAE` sidecars are replicated to active backup drives alongside their master assets.
- **SC-003**: Zero orphan `.AAE` files remain in unorganized source directories when their companion media assets are successfully organized and pruned.
- **SC-004**: Zero data loss or desynchronization occurs for iOS photo/video non-destructive edit instructions during archival.
- **SC-005**: Pipeline execution speed for media sets with `.AAE` sidecars experiences less than 5% overhead compared to media sets without sidecars.

## Assumptions

- Apple `.AAE` sidecar files are XML Property List (plist) files created by iOS Photos app containing non-destructive adjustments; they are small text files (typically 1 KB to 50 KB).
- A companion `.AAE` file resides in the same directory as its source image/video with the same base name (e.g. `IMG_1234.AAE` paired with `IMG_1234.JPG`, `IMG_1234.HEIC`, or `IMG_1234.MOV`).
- In case an `.AAE` file has no companion in the source directory, the system looks up existing primary master assets in the catalog to determine whether the companion was previously ingested.
- `.AAE` files do not require separate perceptual visual hashing (phash) since they are text/plist files, but do require cryptographic hashing (BLAKE3) for transfer verification and deduplication.
