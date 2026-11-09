# Feature Specification: Unified Media Metadata & Fingerprint Scanner

**Feature Branch**: `004-unified-media-scan`  
**Created**: 2026-11-07  
**Status**: Draft  
**Input**: User description: "My ultimate aim is to build a rich metadata JSON about the media files inside all directories for the provided input path, with the feature we build already, the rich metadata EXIF and other details, perceptual hashes, fingerprints, file path, date, etc. whatever the scan does. All the above should be performed with 1 interface or command to perform with 1 mandatory input of file path."

---

## Scope & Intent

Provide a unified, single-interface command that takes a single mandatory file or directory path, recursively discovers all media assets (photos, videos, and camera RAW files) across all nested directories, extracts comprehensive metadata (EXIF, exposure, camera hardware, geolocation, video container atoms, normalized capture dates with calendar facets), computes cryptographic and perceptual visual fingerprints (pHash, rotation hashes, low-entropy guards), and outputs a structured, database-ready JSON record per media asset without requiring multi-command shell pipelines.

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Single-Command Recursive Media Metadata Extraction (Priority: P1) 🎯 MVP

As a media archivist or data engineer, I want to invoke a single command providing only a target folder path, so that all photos, videos, and camera RAW assets in all nested folders are discovered and their full rich metadata (EXIF camera settings, temporal facets, dimensions, video attributes, and file paths) are emitted as structured JSON without needing separate scanning or pipeline piping commands.

**Why this priority**: Users need an intuitive, frictionless entry point. Having to manually pipe separate discovery and extraction commands (`scan | extract-stream`) increases complexity, error rates, and mental overhead. A single command delivering complete metadata provides immediate end-to-end value.

**Independent Test**: Run the unified command targeting a nested folder containing photos and videos; verify that all media files are discovered and each is represented in the output with complete file context, timestamps, calendar facets, and format-specific metadata.

**Acceptance Scenarios**:
1. **Given** a directory containing nested subdirectories of photos and videos, **When** the user runs the unified command with just the directory path, **Then** the system recursively visits every subdirectory, identifies all media assets, and outputs a complete metadata JSON document for each file.
2. **Given** a path pointing directly to a single media file, **When** the command is executed, **Then** the system extracts and displays the rich metadata document for that single file.
3. **Given** an invalid or non-existent path, **When** the command is executed, **Then** the system reports a clear, human-readable error message and exits cleanly.

---

### User Story 2 - Integrated Content Fingerprinting and Perceptual Hashes (Priority: P2)

As a digital asset manager or photo cataloger, I want the unified command to automatically compute cryptographic digests (BLAKE3) and perceptual visual hashes (pHash with rotation variants and blank image detection) for every discovered asset and embed them directly in the metadata JSON, so that downstream search and deduplication systems have identity and similarity markers ready for indexing.

**Why this priority**: Users want content fingerprints and perceptual hashes generated in the exact same pass as EXIF and container parsing. Consolidating fingerprinting into the single scan eliminates redundant disk reads and simplifies downstream ingestion.

**Independent Test**: Run the unified command on a folder containing standard photos, downscaled copies, and camera RAW files; verify that every resulting JSON record contains a populated fingerprint section with full cryptographic hash, fast sample digest, and 64-bit perceptual hash (with rotation variants).

**Acceptance Scenarios**:
1. **Given** any discovered photo, video, or RAW file, **When** extracted by the unified command, **Then** the output document includes the cryptographic hash of the entire file payload and a fast sample digest.
2. **Given** any photo or camera RAW preview, **When** processed, **Then** the output document includes the 64-bit perceptual visual hash (`phash`), its hexadecimal string, orthogonal rotation hashes (`hash_90`, `hash_180`, `hash_270`), and a low-entropy flag indicating whether the frame is blank or monochromatic.
3. **Given** two visually identical photos at different resolutions or orientations, **When** processed by the unified command, **Then** their emitted perceptual hashes allow downstream systems to verify near-duplicate similarity (Hamming distance $\le 10$).

---

### User Story 3 - Flexible Output Options and Live Feedback (Priority: P3)

As a terminal user or workflow automation engineer, I want the unified command to stream newline-delimited JSON (NDJSON) to standard output or save to a specified output file, while displaying optional live progress metrics on processed files and discovery rates, so that I can monitor long-running scans and integrate smoothly with automation scripts.

**Why this priority**: When processing archives containing tens or hundreds of thousands of files across external storage volumes, users need operational visibility into progress, throughput, and error rates, as well as the ability to direct output to files or terminal streams.

**Independent Test**: Execute the unified command with an output file argument on a large test folder; verify that the output file contains valid JSON/NDJSON records for all files while standard error reports real-time progress without polluting standard output.

**Acceptance Scenarios**:
1. **Given** a directory scan, **When** executed with default terminal settings, **Then** output records are emitted to standard output in streaming NDJSON format suitable for redirection into files or database ingestion tools.
2. **Given** an optional output file path argument, **When** provided, **Then** the system writes the extracted metadata JSON records directly into the designated file.
3. **Given** a scan across thousands of media files, **When** running, **Then** the system provides non-intrusive progress updates reporting discovered files, extraction rate, and any skipped files.

---

### Edge Cases

- **Mixed Directories with Non-Media Files**: Folders containing documents, system files (`.DS_Store`, `Thumbs.db`), or source code must be safely skipped without producing metadata documents or errors.
- **Corrupted or Partially Downloaded Files**: Damaged headers or unreadable files must produce a diagnostic status record with failure details in the output JSON, never crashing or stalling the recursive scan.
- **Deeply Nested Folders and Symlink Cycles**: Directory trees with hundreds of nested subdirectories or circular symbolic links must be traversed without stack overflow or infinite loops.
- **Empty Directories or Directories Without Media**: Empty folders must complete gracefully, reporting zero discovered media assets.
- **Camera RAW Files without Embedded Previews**: If preview extraction fails on an unusual RAW variant, the file's EXIF and cryptographic hash must still be captured, with `phash` recorded as null and a warning attached.

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST provide a single unified command interface requiring exactly one mandatory input argument: the target file path or directory path.
- **FR-002**: When provided a directory path, the system MUST recursively traverse all nested subdirectories to discover all supported media files (Photos: JPG, JPEG, PNG, WEBP, GIF, BMP, HEIC, TIFF; Videos: MP4, MOV, MKV, AVI, WEBM, M4V; Camera RAW: CR2, CR3, NEF, ARW, DNG, ORF, RW2).
- **FR-003**: When provided an individual file path, the system MUST directly extract and output the rich metadata document for that single file.
- **FR-004**: System MUST extract rich photographic metadata where present, including camera manufacturer, camera model, lens model, software, aperture, shutter speed, ISO, focal length, pixel dimensions, orientation, and GPS coordinates.
- **FR-005**: System MUST extract video container attributes where present, including duration in milliseconds, display dimensions, frame rate, video codec, audio codec, and container rotation angle.
- **FR-006**: System MUST parse and normalize capture dates into UTC ISO timestamps along with pre-computed integer calendar facets (`year`, `month`, `day`, `hour`, `minute`, `second`), falling back to filesystem timestamps when embedded metadata is missing.
- **FR-007**: System MUST compute a full streaming cryptographic hash (BLAKE3 64-hex characters) and a fast sample digest for every media file without buffering full files into memory.
- **FR-008**: System MUST compute a 64-bit DCT perceptual visual hash (`phash`), hexadecimal string representation (`phash_hex`), orthogonal rotation hashes (90°, 180°, 270°), and low-entropy blank frame detection for photos and camera RAW preview images.
- **FR-009**: System MUST assemble all extracted metadata, file context (absolute path, relative path, file size in bytes, media category), temporal facets, and content fingerprints into a single self-contained JSON document per file.
- **FR-010**: System MUST emit output as streaming newline-delimited JSON (NDJSON) or pretty-printed JSON, ready for direct ingestion into document databases (such as MongoDB).
- **FR-011**: System MUST provide an optional argument to write output directly to a designated file path instead of stdout.
- **FR-012**: System MUST isolate per-file read and decoding errors, recording diagnostic warnings or failure statuses in the file's JSON document while continuing the scan uninterrupted.

---

### Key Entities

- **MediaMetadataDocument**: Complete, self-contained record representing an individual media file.
  - `id`: Unique document identifier (UUID v4).
  - `file_context`: File system path, relative path from scan root, file name, byte size, extension, and category (`Photo`, `Video`, `RawPhoto`).
  - `temporal`: Normalized UTC timestamp, ISO formatted string, and integer calendar facets (`year`, `month`, `day`, `hour`, `minute`, `second`).
  - `camera`: Camera make, model, lens model, firmware version.
  - `exposure`: Aperture, shutter speed, ISO rating, focal length.
  - `location`: Decimal degree latitude, longitude, and altitude.
  - `dimensions`: Width, height, and display orientation.
  - `video`: Duration in milliseconds, codecs, frame rate, container rotation.
  - `fingerprint`: Cryptographic hash, fast sample digest, 64-bit perceptual visual hash (`phash`), hex pHash, rotated variant hashes, and low-entropy flag.
  - `extraction_status`: Success or partial diagnostic status with warnings and error messages.

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users can extract complete metadata for any media archive using a single terminal command with only 1 mandatory path argument.
- **SC-002**: 100% of successfully discovered media assets produce a valid JSON document containing file path, size, media category, capture date with integer calendar facets, and cryptographic fingerprint.
- **SC-003**: 100% of extractable photos and camera RAW files contain a 64-bit perceptual visual hash and rotation variants.
- **SC-004**: Processing throughput exceeds 100 media files per second on standard multi-core hardware.
- **SC-005**: Corrupted, partial, or unreadable files never terminate or abort the scan; 100% of remaining files in the directory tree are processed.
- **SC-006**: Memory consumption remains strictly bounded under 100 MB during traversal and extraction of archives containing over 50,000 files.
- **SC-007**: 100% of emitted documents comply with document database schemas and are immediately ingestible via standard bulk import tools without transformation.

---

## Assumptions

- The target path provided by the user is a readable filesystem location (local volume, external hard drive, or network mount).
- Video files receive cryptographic content digests, dimensions, and duration; visual perceptual hashing focuses on raster images and embedded camera RAW previews.
- Default output format is streaming newline-delimited JSON (NDJSON) on standard output, enabling Unix redirection and database import.
- The command operates non-destructively, performing read-only inspection of source media files.
