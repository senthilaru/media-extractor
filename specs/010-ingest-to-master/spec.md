# Feature Specification: Automated Master Ingestion and Source Deletion Marking

**Feature Branch**: `010-ingest-to-master`

**Created**: 2026-11-09

**Status**: Draft

**Input**: User description: "when a file is marked as NeedsIngestToMaster, i will connect both primary hardrive and other drive to copy contents, whatever files in the other drive needs NeedsIngestToMaster, copy the file and mark for deletion accordingly."

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Automated Ingestion of Unmastered Assets to Primary Master (Priority: P1) 🎯 MVP

As an archivist with both a primary master hard drive and a secondary/external hard drive connected, I want the system to locate all files on the secondary drive marked `NeedsIngestToMaster`, copy them into the primary drive's canonical folder structure (`<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}`), register the newly copied files as `CanonicalMaster` in the database, and mark the original copies on the secondary drive as `MarkForRemoval`, so that all unique media is safely organized into my master archive and ready for cleanup.

**Why this priority**: Core value of the user request. Unique unmastered files scattered across secondary drives must be safely ingested into the primary archive before redundant files can be pruned.

**Independent Test**: Can be tested with a mock secondary drive containing 3 unmastered photos/videos. Running `metadata-extractor ingest --source-drive <DRIVE_ID>` copies the files into the primary drive's structured master folder, indexes the new master records in MongoDB, and updates the source files to `MarkForRemoval`.

**Acceptance Scenarios**:

1. **Given** a secondary drive with files marked `NeedsIngestToMaster` in MongoDB and a primary drive mounted with its master directory accessible, **When** the operator invokes `metadata-extractor ingest --source-drive <DRIVE_ID>`, **Then** each file is copied to its date-and-category-structured canonical path on the primary drive.
2. **Given** a successfully copied file on the primary drive, **When** ingestion completes, **Then** a new or updated database record is created for the primary master path with `dedup.status = "CanonicalMaster"` and the original source record's status is changed from `NeedsIngestToMaster` to `MarkForRemoval` referencing the new master.
3. **Given** the source drive or target primary drive is not mounted at the expected path, **When** the ingest command starts, **Then** the system fails fast with a clear diagnostic message without copying partial files.

---

### User Story 2 - Bitwise Verification & Safe Master Registration (Priority: P2)

As an operator transferring irreplaceable family photos and videos between drives, I want the system to verify the bitwise integrity (size and cryptographic hash) of the copied file on the primary drive before updating database records or marking source files for deletion, so that corrupt or truncated transfers never cause data loss.

**Why this priority**: Guarantees zero data loss. A failed or incomplete copy must never result in the source file being marked for deletion.

**Independent Test**: Simulate an interrupted or corrupted copy (e.g., mismatched file size or byte hash) and verify that the system detects the discrepancy, aborts registration of the master, leaves the source file marked as `NeedsIngestToMaster`, and logs a descriptive transfer error.

**Acceptance Scenarios**:

1. **Given** a copied file on the target drive, **When** the transfer finishes, **Then** the system validates that destination file size and cryptographic hash exactly match the source file before updating the database.
2. **Given** a copy error, disk write failure, or hash mismatch, **When** integrity verification fails, **Then** the corrupted destination file is cleaned up, the source document remains `NeedsIngestToMaster`, and the failure is recorded in the execution summary.

---

### User Story 3 - Ingest Preview & Dry-Run Planning (Priority: P2)

As an operator reviewing pending imports before executing large file transfers across external USB drives, I want to run the ingest command in dry-run mode to inspect the planned source-to-destination mappings, folder dates, and total gigabytes to transfer.

**Why this priority**: Provides operational transparency and safety before initiating multi-gigabyte or multi-hour disk transfers.

**Independent Test**: Run `metadata-extractor ingest --source-drive <DRIVE_ID> --dry-run` and verify that a structured report is emitted detailing the planned file transfers and destination paths while zero files are copied to disk and zero database records are modified.

**Acceptance Scenarios**:

1. **Given** files marked `NeedsIngestToMaster` on a source drive, **When** `ingest` is executed with `--dry-run`, **Then** the terminal outputs a preview table showing each file's source path, proposed canonical master destination path, media category, and byte size.
2. **Given** a dry-run execution, **When** it completes, **Then** neither the file system nor the MongoDB database reflects any changes.

---

### User Story 4 - Post-Ingest Automatic Source Deletion Option (Priority: P3)

As an operator who wants an immediate one-step transfer-and-clean workflow, I want an optional command-line flag (`--delete-source`) that physically deletes the source file from the secondary drive immediately after verified ingestion into the primary master, while defaulting to safe deferred marking (`MarkForRemoval`) when the flag is omitted.

**Why this priority**: Convenient for operators wanting automated space reclamation in a single pass without needing to separately invoke `dedup prune`.

**Independent Test**: Run `metadata-extractor ingest --source-drive <DRIVE_ID> --delete-source` on a test folder; verify that verified files are removed from the source drive, unverified files remain intact, and the source database records are updated to `Removed`.

**Acceptance Scenarios**:

1. **Given** `--delete-source` is supplied, **When** a file is successfully copied, verified, and registered as a canonical master, **Then** the original source file is physically unlinked from the source drive and its database status is marked as `Removed`.
2. **Given** `--delete-source` is omitted (default behavior), **When** ingestion completes, **Then** the source file remains on the source drive and its database status is set to `MarkForRemoval` for future pruning via `dedup prune`.

---

### Edge Cases

- **File Name Collision in Destination Date Folder**: If `IMG_0042.JPG` already exists in `.../master/2021/30-Nov-2021/Photos/`:
  - If content hash matches: the destination file is already identical; link it directly and mark source as `MarkForRemoval`.
  - If content hash differs (different photo with same filename): append a numeric collision suffix (e.g., `IMG_0042_1.JPG`) to prevent overwriting existing master files.
- **Missing or Indeterminate Capture Date**: If a media file has no EXIF or QuickTime temporal metadata, fallback to the file's filesystem modification date to construct the `<Year>/<DD-Mon-YYYY>` folder structure. If neither is available, route to `<MasterRoot>/<Year>/Unsorted/<CategoryFolder>/`.
- **Destination Drive Out of Space**: If the primary master drive runs out of disk space during ingestion, the operation MUST halt safely, leave remaining uncopied source files as `NeedsIngestToMaster`, and emit a clear disk-space warning without corrupting previously completed transfers.
- **Multiple Primary Drives Configured**: If the configuration defines more than one primary drive, the operator MUST be able to specify `--target-drive <DRIVE_ID>`, or default to the highest-priority primary drive.

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST provide an `ingest` CLI command (`metadata-extractor ingest`) that accepts `--source-drive <DRIVE_ID_OR_MOUNT>` and an optional `--target-drive <PRIMARY_DRIVE_ID>`.
- **FR-002**: The system MUST query MongoDB for all documents where `dedup.status == "NeedsIngestToMaster"` whose file path resides on the designated source drive.
- **FR-003**: For each unmastered asset, the system MUST compute the destination path within the target primary drive's master directory following the canonical folder hierarchy: `<MasterRoot>/<Year>/<DD-Mon-YYYY>/<CategoryFolder>/<filename>` where `<CategoryFolder>` is `Videos`, `Photos`, or `PhotosRaw`.
- **FR-004**: The system MUST create target destination directories automatically if they do not exist.
- **FR-005**: The system MUST perform an end-to-end bitwise integrity check (verifying byte size and cryptographic hash) on the copied destination file before registering it in MongoDB.
- **FR-006**: Upon verified copy, the system MUST persist the new canonical master file in MongoDB with `dedup.status = "CanonicalMaster"` and full metadata extracted from the file.
- **FR-007**: Upon verified copy, the system MUST update the original source file's record in MongoDB with `dedup.status = "MarkForRemoval"` and link its `canonical_master_id` and `canonical_master_path` to the newly created master.
- **FR-008**: The system MUST support a `--dry-run` flag that previews all planned copy operations, destination paths, and total transfer sizes without touching disk or database state.
- **FR-009**: The system MUST support an optional `--delete-source` flag that unlinks the source file upon successful verification and updates its database status to `Removed`.
- **FR-010**: If a file with the same name exists at the destination with differing content, the system MUST resolve collisions by appending an incrementing suffix (e.g. `<stem>_1.<ext>`) to guarantee non-destructive ingestion.
- **FR-011**: The system MUST emit real-time progress indicators showing bytes copied, items transferred, current transfer rate, and elapsed time during the ingestion process.
- **FR-012**: If any individual file transfer fails or is aborted, the system MUST isolate the failure, leave the source file as `NeedsIngestToMaster`, and continue processing remaining items.

---

### Key Entities

- **MasterIngestPlan**: An evaluated transfer plan containing:
  - `source_drive_id`: Identifier of the drive providing unmastered content.
  - `target_drive_id`: Identifier of the primary master drive receiving content.
  - `items_to_ingest`: List of planned ingest tasks.
  - `total_bytes_to_transfer`: Total byte volume across all planned tasks.
- **IngestTask**: A single transfer operation containing:
  - `source_doc_id`: UUID of the source document in MongoDB.
  - `source_file_path`: Path on the source drive.
  - `target_file_path`: Computed canonical path on the primary master drive.
  - `file_size_bytes`: Size of the asset.
  - `category`: `Photo`, `RawPhoto`, or `Video`.
  - `target_folder_date`: Formatted date folder (`DD-Mon-YYYY`).
- **IngestExecutionReport**: Summary returned after ingestion completes:
  - `transferred_count`: Number of files successfully copied and verified.
  - `transferred_bytes`: Total byte volume successfully written.
  - `failed_count`: Number of files that encountered transfer or verification errors.
  - `elapsed_seconds`: Total execution duration.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of files marked `NeedsIngestToMaster` on the source drive are transferred to the correct `<Year>/<DD-Mon-YYYY>/<CategoryFolder>` path without operator manual sorting.
- **SC-002**: Zero data loss guarantee: 100% of copied files pass post-copy cryptographic integrity verification before source files are marked for removal.
- **SC-003**: Filename collisions with differing file content result in zero overwritten files.
- **SC-004**: Dry-run mode accurately calculates and displays 100% of planned target paths and byte volumes without disk writes or database state mutations.
- **SC-005**: High throughput sustained transfer utilizing streaming I/O buffers matching the host storage bandwidth limits.

---

## Assumptions

- Both the source external drive and target primary external drive are connected and mounted concurrently during the ingest execution.
- The primary drive has sufficient free disk space to accommodate the incoming unmastered media.
- The user maintains `drives.yaml` with at least one `primary` group drive specifying a valid `master_path`.
- Media files contain valid capture dates in EXIF/QuickTime headers or fall back reliably to file system modification dates.
