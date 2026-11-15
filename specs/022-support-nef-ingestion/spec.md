# Feature Specification: Ingest Nikon NEF Raw Format

**Feature Branch**: `022-support-nef-ingestion`

**Created**: 2026-11-15

**Status**: Draft

**Input**: User description: "the file format NEF from Nikon camera is not ingested. I want to add this"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Discover and Ingest Nikon NEF Files into Master Archive (Priority: P1)

As a media archivist with Nikon digital camera RAW files (`.NEF` / `.nef`) on an external source drive, I want the system to recognize, extract metadata from, and ingest these files into the primary master storage structure so that all my Nikon photos are properly archived alongside other media formats.

**Why this priority**: Core user need. Without this, Nikon camera RAW images cannot be archived or organized into the master storage hierarchy, leaving unmastered assets stranded on secondary or temporary media.

**Independent Test**: Connect a source volume containing Nikon `.NEF` files and run the organization/ingestion process. Verify that each `.NEF` file is copied to its chronological destination under `<Year>/<DD-Mon-YYYY>/PhotosRaw/<filename>` on the primary master volume, registered in the catalog as a canonical master, and verified bit-for-bit.

**Acceptance Scenarios**:

1. **Given** unmastered Nikon `.NEF` files on a connected source drive, **When** the ingestion pipeline executes, **Then** all `.NEF` files are copied to the primary master drive under `<Year>/<DD-Mon-YYYY>/PhotosRaw/<filename>` using capture dates extracted from Nikon EXIF tags.
2. **Given** an ingested `.NEF` file on the primary master drive, **When** checking the catalog database, **Then** the file is recorded with category `RawPhoto`, status `CanonicalMaster`, and verified cryptographic content hash.
3. **Given** an unmastered `.NEF` file whose previous scan was incomplete or failed, **When** scanning or organizing again, **Then** the pipeline detects that the record requires completion and re-extracts full metadata rather than skipping it.

---

### User Story 2 - Accurate Full Sensor Metadata Extraction for NEF Files (Priority: P2)

As a photographer, I want the system to extract the true sensor dimensions (e.g., 6000x4000) and camera exposure settings from Nikon NEF files rather than reading embedded low-resolution thumbnail preview headers (e.g., 160x120), so that catalog resolution analysis, quality ranking, and EXIF health reports accurately represent the full-fidelity RAW images.

**Why this priority**: Resolution-aware deduplication and quality ranking rely on image width and height. Extracting 160x120 thumbnail dimensions causes high-resolution RAW files to be misranked or flagged as low-resolution.

**Independent Test**: Run extraction on a Nikon D7100 `.NEF` file. Verify that the recorded image dimensions reflect the full sensor resolution (6000x4000) and that camera model (`NIKON D7100`), ISO, aperture, and shutter speed are correctly captured.

**Acceptance Scenarios**:

1. **Given** a Nikon `.NEF` file with an embedded preview thumbnail, **When** metadata extraction is performed, **Then** the extracted dimensions reflect the primary full-resolution RAW image rather than the thumbnail dimensions.
2. **Given** a `.NEF` file with valid camera EXIF, **When** capture date is extracted, **Then** the timestamp source is identified as camera original capture datetime (`ExifDateTimeOriginal`).

---

### User Story 3 - Replica Synchronization and Safe Source Pruning for NEF (Priority: P3)

As an archivist maintaining redundant backups, I want Nikon NEF files ingested into the primary master drive to automatically synchronize to all configured backup replica drives and safely prune from the source drive only after verification.

**Why this priority**: Guarantees zero data loss and ensures the 3-2-1 backup strategy applies uniformly to Nikon RAW files.

**Independent Test**: Ingest a batch of Nikon `.NEF` files and synchronize replicas. Verify that the exact same files exist on replica drives under `<Year>/<DD-Mon-YYYY>/PhotosRaw/`, with matching cryptographic digests, and that source copies are pruned only after master and replica verification.

**Acceptance Scenarios**:

1. **Given** a `.NEF` file ingested into primary master, **When** replica synchronization runs, **Then** the `.NEF` file is mirrored to each active backup replica maintaining identical relative folder structure and content hash.
2. **Given** verified master and replica copies of a `.NEF` file, **When** source pruning executes, **Then** the original source copy on the secondary drive is safely deleted and empty directories are cleaned up.

---

### Edge Cases

- What happens when a Nikon camera shoots both a `.NEF` and `.JPG` with the same base name (e.g., `DSC_0001.NEF` and `DSC_0001.JPG`)?
  - The system must treat both as distinct valid assets: the `.JPG` is organized into `Photos/` and the `.NEF` is organized into `PhotosRaw/`, neither overwriting nor pruning the other as a false duplicate.
- What happens when a `.NEF` file is corrupted or partially transferred?
  - The system routes the corrupted file to a failed extraction status with clear diagnostic warnings and does not transfer or mark it as canonical master.
- What happens when a `.NEF` file lacks EXIF capture timestamps?
  - The system falls back deterministically to file modification time and records a missing EXIF metadata warning, continuing safe ingestion.
- What happens when a `.NEF` file was previously scanned in fast mode or aborted mid-run?
  - Incremental filtering must detect that the document lacks a cryptographic content fingerprint or is in a failed status and schedule it for re-extraction.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST identify and classify Nikon camera raw files with `.nef` and `.NEF` extensions under the `RawPhoto` media category.
- **FR-002**: System MUST extract full-resolution image dimensions from Nikon NEF raw container structures, prioritizing full-sensor raw dimensions over embedded thumbnail preview dimensions.
- **FR-003**: System MUST extract camera capture timestamps (`DateTimeOriginal`), camera make/model, and exposure metadata from Nikon NEF files.
- **FR-004**: System MUST compute cryptographic content hashes for Nikon NEF files according to configured performance profiles.
- **FR-005**: System MUST assign `NeedsIngestToMaster` deduplication status to unique unmastered NEF files discovered on external source drives.
- **FR-006**: Ingestion engine MUST route NEF files into the primary master directory using the canonical hierarchy `<MasterRoot>/<Year>/<DD-Mon-YYYY>/PhotosRaw/<filename>`.
- **FR-007**: System MUST replicate ingested NEF files to all configured replica backup drives, preserving path parity and bitwise verification.
- **FR-008**: System MUST protect NEF files from being falsely identified as duplicates of companion JPEG files with identical base stems.
- **FR-009**: Incremental scanning pre-filter MUST re-queue existing catalog entries for extraction if their previous extraction status was failed or if their cryptographic content fingerprint is missing.
- **FR-010**: Pruning operations MUST safely delete source NEF files only after bitwise verification on master and required replica destinations.

### Key Entities

- **Nikon RAW Asset (NEF)**: Represents a raw sensor capture from a Nikon digital camera (`.nef`/`.NEF`), classified under `RawPhoto`, containing uncompressed or lossless raw pixel data, camera metadata, and full-resolution dimensions.
- **Canonical Master Record**: Catalog document registering the verified NEF asset in primary master storage (`PhotosRaw/`), including dimensions, capture timestamp, camera metadata, and replication lifecycle state.
- **Replica Backup Record**: Catalog document tracking mirrored copies of the NEF asset across secondary and cold backup storage tiers.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of valid Nikon `.NEF` files discovered on a source drive are successfully extracted, categorized as `RawPhoto`, and ingested into primary master storage under `<Year>/<DD-Mon-YYYY>/PhotosRaw/`.
- **SC-002**: 100% of extracted NEF files record true full-sensor dimensions (e.g. $\ge 1000$ pixels on shortest edge) rather than thumbnail preview dimensions ($\le 320$ pixels).
- **SC-003**: 100% of ingested NEF files replicate to configured replica drives with identical byte size and bitwise cryptographic hash before source pruning.
- **SC-004**: Zero data loss: no original NEF file is pruned from a source volume without a verified primary master and replica copy.
- **SC-005**: Ingestion throughput for NEF files achieves parity with standard RAW processing rates ($\ge 30$ MB/s on USB 3.0 / external drives).

## Assumptions

- Nikon NEF files follow standard TIFF/EP and Nikon Electronic Format container conventions containing embedded EXIF IFDs and RAW sub-IFDs.
- Ingestion destination for `RawPhoto` category media adheres to the project standard subfolder `PhotosRaw/` within date-based folders (`<Year>/<DD-Mon-YYYY>/PhotosRaw/`).
- Same-stem companion files (e.g. `DSC_0001.NEF` and `DSC_0001.JPG`) are intentional dual-format captures by the photographer and must both be preserved in their respective category folders (`PhotosRaw/` and `Photos/`).
- Files previously cataloged with `Failed` extraction status or missing fingerprints were interrupted or unmounted and must be re-evaluated during subsequent passes.
