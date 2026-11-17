# Feature Specification: Support for VOB Video Extensions and DVD Metadata Files

**Feature Branch**: `023-support-vob-metadata`

**Created**: 2026-11-16

**Status**: Draft

**Input**: User description: "Add support for VTS_01_1.VOB extensions and its metadata files."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Media Discovery & Classification for VOB Video and DVD Metadata Files (Priority: P1) 🎯 MVP

As a media archivist scanning a backup drive containing DVD rips or camera archives with `VTS_01_1.VOB` files, I want the scanner to recognize `.vob` as a video file (`MediaCategory::Video`) and its companion metadata files (`.ifo` and `.bup`) as sidecar metadata files (`MediaCategory::Sidecar`), so that they are indexed into MongoDB with their cryptographic fingerprints and metadata.

**Why this priority**: Without classification support, DVD media files and their navigation metadata files are ignored or unhandled during scanning, preventing them from being inventoried, deduplicated, or archived.

**Independent Test**: Scan a directory containing `VTS_01_1.VOB`, `VTS_01_0.IFO`, and `VTS_01_0.BUP`. Verify that the scanner discovers all three files, catalogs the `.vob` as `MediaCategory::Video`, catalogs `.ifo` and `.bup` as `MediaCategory::Sidecar`, and stores them in MongoDB.

**Acceptance Scenarios**:

1. **Given** a directory containing `VTS_01_1.VOB` and `VIDEO_TS.VOB`, **When** media discovery executes, **Then** both files are classified as `MediaCategory::Video` regardless of extension casing (`.vob`, `.VOB`).
2. **Given** a directory containing `VTS_01_0.IFO` and `VTS_01_0.BUP`, **When** media discovery executes, **Then** both files are classified as `MediaCategory::Sidecar` regardless of extension casing (`.ifo`, `.IFO`, `.bup`, `.BUP`).
3. **Given** a `.vob` file without embedded container tags, **When** metadata extraction runs, **Then** the video metadata extractor extracts file attributes, assigns default MPEG-2 video codec indicators, and falls back gracefully to filesystem creation/modification timestamps.

---

### User Story 2 - Ingestion & Co-Location of VOB Videos and Companion DVD Metadata (Priority: P1)

As an archivist organizing media into the canonical primary master archive, I want `VTS_01_1.VOB` to be ingested into `<Year>/<DD-Mon-YYYY>/Videos/`, and its companion DVD metadata files (`.ifo` and `.bup`) to be co-located directly beside the video in the same `Videos` folder, so that DVD playback structures and chapter/title information remain intact.

**Why this priority**: DVD metadata files (`.ifo`, `.bup`) belong directly with their video title set. Co-locating them in the same `Videos` folder guarantees that media players and archivists have all required title set streams and navigation structures in one place.

**Independent Test**: Ingest a folder containing `VTS_01_1.VOB`, `VTS_01_0.IFO`, and `VTS_01_0.BUP`. Verify that all three files are transferred to the same canonical master date folder under `Videos/`.

**Acceptance Scenarios**:

1. **Given** `VTS_01_1.VOB` and its companion `VTS_01_0.IFO` and `VTS_01_0.BUP` pending ingest, **When** master ingest executes, **Then** `VTS_01_1.VOB` is copied to `<Year>/<DD-Mon-YYYY>/Videos/` and both `VTS_01_0.IFO` and `VTS_01_0.BUP` are placed alongside it in the same `Videos/` directory.
2. **Given** `VTS_01_1.VOB` already exists in canonical master, **When** a delayed companion `VTS_01_0.IFO` or `VTS_01_1.IFO` is ingested, **Then** the sidecar resolves the existing master video's location and is placed in that master's `Videos/` folder.
3. **Given** an exact-name companion pair `VTS_01_1.VOB`, `VTS_01_1.IFO`, `VTS_01_1.BUP` or `VIDEO_TS.VOB`, `VIDEO_TS.IFO`, `VIDEO_TS.BUP`, **When** ingested, **Then** all files are placed in the same `Videos/` folder.

---

### User Story 3 - Replica Synchronization & Pruning of VOB Media and Metadata (Priority: P2)

As a system maintaining full backup redundancy and clean staging drives, I want `VTS_01_1.VOB` and all companion `.ifo`/`.bup` files to be synchronized to replica drives, and safely pruned from source staging folders once verified in primary master, so that storage is reclaimed and empty directories are removed.

**Why this priority**: Ensures 100% data parity across backup replicas and allows complete source cleanup so DVD folder hierarchies don't leave ghost directories or orphaned files.

**Independent Test**: Execute organize or prune on a source folder containing ingested `VTS_01_1.VOB`, `VTS_01_0.IFO`, and `VTS_01_0.BUP`. Verify all three are mirrored to replicas, safely pruned from source, and the containing directory is cleaned.

**Acceptance Scenarios**:

1. **Given** canonical master contains `VTS_01_1.VOB` and its companion `.ifo`/`.bup` files, **When** `sync-replicas` executes, **Then** the video and metadata files are mirrored to all active replica drives.
2. **Given** source directory contains redundant `VTS_01_1.VOB` and companion `.ifo`/`.bup` files, **When** prune executes, **Then** all verified files are unlinked and the source directory is removed.

---

### Edge Cases

- **Multiple companion sidecars**: A single video may have both `.ifo` and `.bup` companion files (and potentially an `.aae` or `.xmp` file). The system must discover and transfer ALL companion files, not just the first one found.
- **DVD Title Set stem conventions**: Stems can be exact match (`VTS_01_1.VOB` and `VTS_01_1.IFO`), title set root (`VTS_01_1.VOB` and `VTS_01_0.IFO` / `VTS_01_0.BUP`), or disc root (`VIDEO_TS.VOB`, `VIDEO_TS.IFO`, `VIDEO_TS.BUP`). The companion matching logic must recognize these DVD naming patterns.
- **Standalone VOB or metadata**: If a `.vob` has no `.ifo`/`.bup`, it is still processed as a valid video. If an `.ifo`/`.bup` has no `.vob`, it is ingested safely into `Videos/` based on its own timestamp.
- **Mixed case extensions**: Files created on legacy Windows/FAT32/UDF systems may have `.VOB`, `.vob`, `.IFO`, `.ifo`, `.BUP`, `.bup`. Classification and matching must be strictly case-insensitive.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST classify files with `.vob` extension (case-insensitive) as `MediaCategory::Video`.
- **FR-002**: The system MUST classify files with `.ifo` and `.bup` extensions (case-insensitive) as `MediaCategory::Sidecar`.
- **FR-003**: The video extraction engine MUST support `.vob` files, extracting file context, setting default video codec to MPEG-2 (`mpeg2`) and audio codec to `ac3/lpcm/dts`, and falling back gracefully to container/filesystem timestamps.
- **FR-004**: The transfer service MUST support discovering multiple companion sidecars (`find_companion_sidecars`) for a media asset, including `.ifo`, `.bup`, and `.aae`.
- **FR-005**: The sidecar stem matcher MUST recognize DVD Title Set naming conventions where `VTS_xx_y.VOB` matches `VTS_xx_0.IFO`, `VTS_xx_0.BUP`, `VTS_xx_y.IFO`, and `VTS_xx_y.BUP`, as well as `VIDEO_TS` sets.
- **FR-006**: When ingesting companion sidecars associated with video assets, the ingestion service MUST place the sidecars in the `Videos` category directory (`<Year>/<DD-Mon-YYYY>/Videos/`).
- **FR-007**: Replica synchronization MUST mirror `.vob`, `.ifo`, and `.bup` files to all active replica drives.
- **FR-008**: The pruning service MUST verify that `.vob` and companion `.ifo`/`.bup` files exist on the canonical master before deleting them from source disks.

### Key Entities

- **DVD Media Asset (`.vob`)**: Video object container holding MPEG-2 video streams, categorized as `MediaCategory::Video`.
- **DVD Metadata Sidecars (`.ifo`, `.bup`)**: Information and backup navigation files associated with DVD title sets, categorized as `MediaCategory::Sidecar`.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of `.vob` files are discovered and ingested as `MediaCategory::Video` into the `Videos` master hierarchy.
- **SC-002**: 100% of `.ifo` and `.bup` companion metadata files are co-located in the same `Videos` folder alongside their primary `.vob` media files.
- **SC-003**: Deduplication, replication, and pruning process `.vob`, `.ifo`, and `.bup` files with zero data loss and 100% replica parity.
- **SC-004**: Source directories containing only ingested `.vob`, `.ifo`, and `.bup` files are cleanly pruned and removed after organization.

## Assumptions

- `.vob` files conform to standard DVD-Video / MPEG-2 Program Stream formats.
- `.ifo` and `.bup` files are small navigation/metadata sidecars that accompany DVD video streams.
- If no embedded timestamp can be extracted from a `.vob` container, falling back to filesystem modification time is standard behavior.
