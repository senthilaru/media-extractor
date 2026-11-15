# Feature Specification: Resolution-Aware Deduplication Quality Selection

**Feature Branch**: `021-prefer-highest-resolution`

**Created**: 2026-11-14

**Status**: Draft

**Input**: User description: "while mark the file as dedup or markTodelete, make sure to keep the high resolution image and delete the low resolution images or small images"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Prefer Highest Resolution in Duplicate Groups (Priority: P1)

As a media library owner, when scanning duplicate or visually matching media across my drives, I want the system to automatically select the highest resolution (largest pixel dimension) file as the canonical keeper and designate lower-resolution copies (such as thumbnails, resized previews, or downscaled web copies) as duplicates marked for removal, so that my archive always preserves maximum visual fidelity.

**Why this priority**: Preserving the highest visual quality is the central objective of media archival. Keeping a low-resolution thumbnail while deleting a high-resolution original causes irreversible quality loss.

**Independent Test**: Can be tested by running deduplication on a test set containing identical visual images at 4000x3000, 1920x1080, and 300x300 dimensions. The 4000x3000 image must be selected as the keeper (`CanonicalMaster` or `NeedsIngestToMaster`), while the 1920x1080 and 300x300 copies are marked `MarkForRemoval`.

**Acceptance Scenarios**:

1. **Given** a cluster of duplicate images with varying pixel dimensions, **When** deduplication evaluates the cluster, **Then** the image with the highest total pixel count (width × height) is chosen as the keeper asset.
2. **Given** a high-resolution original and one or more downscaled/thumbnail copies, **When** deduplication runs, **Then** all lower-resolution copies are assigned status `MarkForRemoval` with their byte counts credited to reclaimable storage.
3. **Given** duplicate video files with different frame dimensions (e.g., 4K 3840x2160 vs 1080p 1920x1080), **When** deduplication evaluates the videos, **Then** the higher-resolution video stream is preserved as the keeper.

---

### User Story 2 - File Size Tie-Breaking for Equal Resolution (Priority: P2)

As a media curator, when multiple duplicate files have identical pixel dimensions (e.g., two 1920x1080 images), I want the system to preserve the larger file (higher compression quality, less aggressive JPEG quantization, or lossless encoding) and mark the smaller, heavily compressed file for deletion.

**Why this priority**: When dimensions match, file size directly correlates with image fidelity and compression artifacts. Preserving the larger file retains higher dynamic range and fine detail.

**Independent Test**: Can be tested with two 1920x1080 JPEG images where one is saved at 95% quality (3.2 MB) and one at 60% quality (450 KB). Deduplication must choose the 3.2 MB file as the keeper and mark the 450 KB file for removal.

**Acceptance Scenarios**:

1. **Given** two or more duplicate assets with identical pixel width and height, **When** deduplication evaluates the group, **Then** the asset with the largest file size in bytes is selected as the keeper.
2. **Given** two duplicate assets with identical pixel dimensions and identical file sizes, **When** deduplication evaluates the group, **Then** the system uses secondary metadata completeness (e.g., presence of EXIF/GPS tags) followed by drive priority hierarchy to determine the keeper deterministically.

---

### User Story 3 - Upgrading Lower-Resolution Assets in Master Archive (Priority: P3)

As an archive administrator, when scanning an external drive that contains a higher-resolution version of an image that was previously ingested into the master archive at a lower resolution, I want the system to identify that a higher-quality copy is available so that the master archive can be upgraded without losing the better asset.

**Why this priority**: Users frequently ingest older phone backups or social media exports containing thumbnails, then later locate original camera memory cards with full-resolution originals. The archive must elevate the superior copy.

**Independent Test**: Can be tested by having an existing master record with a 1024x768 image on the primary master drive, then introducing a 4032x3024 copy on a staging drive. The system must recognize the 4032x3024 file as superior and prepare it for ingestion.

**Acceptance Scenarios**:

1. **Given** an existing master asset on the primary drive and an incoming candidate on an external drive with strictly higher resolution, **When** deduplication evaluates the match, **Then** the system marks the higher-resolution incoming candidate for ingest to master according to the configured master upgrade policy.

---

### Edge Cases

- **Missing or Unextractable Dimensions**: When media files lack dimensional metadata (e.g., unsupported vector files, audio files, corrupt EXIF headers), the system falls back to file size in bytes, followed by EXIF timestamp, then drive hierarchy.
- **Cropped Variants with Higher Resolution**: If a cropped detail has high pixel dimensions but represents a fraction of the scene, visual perceptual hashing thresholds prevent grouping unlike compositions together.
- **RAW vs JPEG Companion Formats**: When a duplicate group contains a RAW camera negative (e.g., `.CR2`, `.NEF`, `.DNG`) and an extracted embedded JPEG preview with different dimensions, the system prioritizes RAW preservation as the master negative regardless of pixel dimensions.
- **Identical Resolution and File Size Across Drives**: When two files have identical dimensions, file size, and hashes, canonical keeper selection preserves existing drive priority (`Primary Master` > `Replica` > `Staging/Unorganized`).

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST extract and record media visual dimensions (width in pixels and height in pixels) for all supported image and video formats during catalog ingestion and metadata extraction.
- **FR-002**: During duplicate group evaluation (both exact bitwise duplicates and visual near-duplicate clusters), the system MUST rank candidates within each cluster using a quality-first ordering hierarchy:
  1. Total pixel area (`width × height`) descending.
  2. File size in bytes descending (for equal pixel area).
  3. Metadata completeness score (presence of EXIF date, camera make/model, GPS coordinates) descending.
  4. Drive priority hierarchy (`Primary Master` > `Replica` > `External / Staging`).
- **FR-003**: The candidate with the highest ranking according to FR-002 MUST be designated as the keeper asset (`CanonicalMaster` if already on primary master, or `NeedsIngestToMaster` if not yet on primary master).
- **FR-004**: All other candidates within the duplicate cluster with lower ranking MUST be marked `MarkForRemoval` if their keeper asset is verified, and their file sizes must be aggregated into total reclaimable storage.
- **FR-005**: If a candidate marked for removal is located on a primary master or replica drive, programmatic deletion MUST remain forbidden in accordance with the archive protection circuit breaker; the file path MUST instead be emitted to a manual deletion manifest.
- **FR-006**: When an unmastered candidate has higher resolution than an existing copy already located on the primary master drive, the system MUST automatically designate the higher-resolution candidate as `NeedsIngestToMaster` to upgrade the master archive, and upon verified ingest, queue the inferior lower-resolution master file to the manual deletion manifest (`manual_delete.txt`).
- **FR-007**: When candidate images in a cluster have a conflict between resolution and metadata (e.g., a high-resolution image stripped of EXIF vs a lower-resolution image retaining full EXIF/GPS), the system MUST prioritize pixel resolution as the primary quality determinant, selecting the high-resolution file as the keeper.
- **FR-008**: Deduplication and organize summary reports MUST explicitly indicate the count and total reclaimed bytes of lower-resolution and smaller files marked for pruning.

### Key Entities *(include if feature involves data)*

- **MediaAssetDimensions**: Represents visual geometry of a media file:
  - `width`: Pixel width (integer).
  - `height`: Pixel height (integer).
  - `total_pixels`: Computed total pixel count (`width * height`).
- **MediaQualityRank**: Computed quality score for deduplication ranking:
  - `pixel_count`: Total pixels.
  - `file_size_bytes`: Byte length.
  - `has_exif_date`: Boolean flag.
  - `has_gps`: Boolean flag.
  - `drive_priority`: Numerical weight of current storage location.
- **DeduplicationCluster**: Group of identical or visually matching media items:
  - `cluster_id`: Unique identifier for the duplicate group.
  - `keeper_id`: Document ID of the chosen highest-quality asset.
  - `prune_candidate_ids`: List of document IDs for inferior/redundant copies.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of duplicate clusters evaluated by deduplication select the highest-resolution asset as the keeper.
- **SC-002**: When identical resolutions are present, 100% of duplicate clusters select the largest file size asset as the keeper.
- **SC-003**: Zero instances of a higher-resolution original being marked for deletion when a lower-resolution duplicate exists in the catalog.
- **SC-004**: Deduplication evaluation throughput completes within 5% of existing classification runtime without regressing batch I/O performance.

## Assumptions

- Image dimensions are extracted during standard metadata extraction and stored in the catalog document (`metadata.dimensions.width` and `metadata.dimensions.height`).
- Video frame dimensions are extracted via container parser and stored in media metadata.
- If dimensions are absent or unreadable, the system safely falls back to byte size ranking to prevent classification failures.
- Primary master protection rules established in prior specifications remain inviolable: primary master files can never be deleted programmatically by automated pruning, only queued in manual manifests.
