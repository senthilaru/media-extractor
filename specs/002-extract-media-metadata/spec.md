# Feature Specification: Media Metadata Extraction Engine

**Feature Branch**: `002-extract-media-metadata`

**Created**: 2026-11-07

**Status**: Draft

**Input**: User description: "after the scanning the folders for media files, i want to Extract EXIF and rich metadata across compressed and RAW photos/videos from Android, iPhone, DSLRs, and other devices. Media metadata extraction engine supporting EXIF, RAW photo decoders, and video container metadata.Maintain the scanned folder, path, medatadata, datatime, year, month, file path, all critical info of the file, in a JSON output format, so that it can later save it a database like mongoDB"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Comprehensive Photo & RAW Metadata Extraction (Priority: P1) 🎯 MVP

As a media library operator, I want the system to parse embedded EXIF, TIFF, and XMP metadata from compressed images (JPEG, PNG, HEIC, WebP) and camera RAW files (Canon CR2/CR3, Nikon NEF, Sony ARW, Adobe DNG, Fujifilm RAF, Olympus ORF) without decompressing full image payloads, so that capture dates, camera hardware, exposure settings, GPS locations, and image dimensions are extracted with high fidelity and speed.

**Why this priority**: Photo archives from smartphones and DSLRs make up the vast majority of media files. Extracting rich photographic metadata and calendar facets is the core value proposition for cataloging and search.

**Independent Test**: Point the extractor at a directory containing sample iPhone HEIC photos, Android JPEG photos, and DSLR RAW images; verify that capture timestamps, calendar facets (`year`, `month`, `day`), camera body/lens, exposure parameters, and dimensions are accurately extracted into the standardized schema.

**Acceptance Scenarios**:

1. **Given** a smartphone photo (e.g., iPhone HEIC or Android JPEG) with standard EXIF headers, **When** metadata extraction is performed, **Then** the exact capture datetime, derived calendar facets (`year`, `month`, `day`), camera make/model, exposure settings, image dimensions, and GPS coordinates are accurately extracted.
2. **Given** a professional DSLR or mirrorless RAW image (e.g., Canon `.CR3`, Nikon `.NEF`, Sony `.ARW`, Adobe `.DNG`), **When** metadata extraction is performed, **Then** camera body, lens model, focal length, aperture, shutter speed, ISO rating, and sensor dimensions are extracted without decompressing full pixel raster data.
3. **Given** an image file stripped of embedded EXIF dates (e.g. web downloads or screenshots), **When** extracted, **Then** the system falls back to the filesystem modification timestamp, populates the calendar facets, and flags the timestamp source as a filesystem fallback.

---

### User Story 2 - Video Container Metadata Parsing (Priority: P2)

As a media cataloger, I want the system to inspect video container headers (MP4, MOV, MKV, WebM, 3GP) to extract recording timestamps, playback duration, video/audio codecs, resolution, frame rate, and orientation tags, so that video assets are cataloged with the same depth as photo assets.

**Why this priority**: Modern personal media libraries consist of substantial video collections. Parsing video headers is required for comprehensive chronological indexing and deduplication.

**Independent Test**: Point the extractor at test video clips from Android and iPhone devices (MP4 and MOV containers); verify that duration in milliseconds, display dimensions, video codec, frame rate, and container recording timestamps are correctly extracted.

**Acceptance Scenarios**:

1. **Given** a video file (MP4 or MOV) recorded on a mobile device or camera, **When** metadata extraction runs, **Then** the playback duration, display resolution, video codec, frame rate, and recording datetime are captured into the normalized document.
2. **Given** a smartphone video recorded vertically with a 90-degree container rotation tag, **When** extracted, **Then** the rotation angle and effective display dimensions are correctly identified.
3. **Given** a multi-gigabyte video file (> 2 GB), **When** extracted, **Then** container metadata is read rapidly in under 50 milliseconds by streaming container atom headers without loading entire video stream bytes.

---

### User Story 3 - Normalized JSON Export for Database Ingestion (Priority: P3)

As a database administrator, I want all extracted media information to be emitted in a strictly validated, normalized JSON document structure containing folder context, file paths, calendar facets, technical specifications, and error diagnostics, so that records can be directly loaded into document databases like MongoDB without custom transformation.

**Why this priority**: Downstream deduplication and search workflows require a unified, queryable data format where paths, dates, and device specs are structured identically regardless of original media format.

**Independent Test**: Run extraction on a mixed batch of photos, videos, RAW files, and corrupted headers; verify that all emitted records conform to the JSON schema, validate that required date facets (`year`, `month`, `day`) are present as integers, and ensure corrupted files output valid diagnostic records rather than terminating the process.

**Acceptance Scenarios**:

1. **Given** a batch of processed media items, **When** JSON output is emitted, **Then** each document contains root directory path, relative path, canonical file path, file size in bytes, derived calendar facets (`year`, `month`, `day`), and nested metadata blocks.
2. **Given** a media file with partially unreadable, malformed, or corrupt metadata, **When** processed, **Then** the system extracts all valid attributes, records diagnostic error messages in the extraction status block, and outputs a valid JSON document.
3. **Given** a pipeline streaming output, **When** formatting as newline-delimited JSON (NDJSON), **Then** each line is an independently parseable JSON object suitable for bulk database import.

---

### Edge Cases

- **Non-Standard Date Formats**: EXIF date strings with non-standard separators (e.g. `YYYY:MM:DD HH:MM:SS`, ISO 8601, or missing timezone offsets) must be parsed robustly into UTC timestamps.
- **Large Video Header Locations**: Video files where the `moov` atom is located at the end of the file rather than the beginning must be parsed efficiently using bounded seeking without loading intermediate video frames.
- **Corrupt MakerNotes**: Proprietary camera manufacturer notes with invalid pointer offsets must be quarantined without aborting extraction of standard EXIF/TIFF tags.
- **Zero-Byte or Truncated Files**: Files that terminate before complete header sequences must emit a diagnostic extraction failure record with basic file attributes.
- **Invalid GPS Data**: EXIF coordinates with zero or out-of-bounds latitude/longitude values (e.g. `0.0, 0.0` or $>90^\circ$) must be validated and sanitized or flagged as invalid.
- **Non-UTF-8 Metadata Strings**: Camera make/model or copyright fields containing raw byte encodings (e.g. Shift-JIS or Latin-1) must be losslessly converted or sanitized into valid UTF-8 JSON strings.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST accept discovered media items (file paths, detected categories, byte sizes) and extract comprehensive technical and descriptive metadata.
- **FR-002**: System MUST parse embedded EXIF, TIFF, and XMP metadata from compressed photo formats, including JPEG, PNG, HEIC, and WebP.
- **FR-003**: System MUST parse metadata from professional camera RAW formats, including Canon (`CR2`, `CR3`), Nikon (`NEF`), Sony (`ARW`), Adobe (`DNG`), Fujifilm (`RAF`), and Olympus (`ORF`).
- **FR-004**: System MUST parse container metadata from video formats, including MP4, MOV, MKV, AVI, and WebM.
- **FR-005**: System MUST extract and normalize capture timestamps into ISO 8601 UTC format, and derive discrete integer calendar facets: `year` (e.g. 2026), `month` (1–12), `day` (1–31), `hour`, `minute`, and `second`.
- **FR-006**: System MUST fall back to file system timestamps when embedded capture metadata is missing or corrupted, setting a flag indicating the timestamp source.
- **FR-007**: System MUST extract camera hardware specifications when available, including manufacturer (`make`), camera model (`model`), lens model, and software/firmware version.
- **FR-008**: System MUST extract photographic capture settings when available, including focal length (mm and 35mm equivalent), aperture ($f$-number), shutter speed / exposure time, ISO rating, and flash mode.
- **FR-009**: System MUST extract GPS spatial coordinates (latitude, longitude, altitude) and normalize them into signed decimal degrees.
- **FR-010**: System MUST extract video container attributes, including playback duration in milliseconds, display width, display height, frame rate (fps), video codec, audio codec, and rotation angle.
- **FR-011**: System MUST output extracted metadata in a consistent, normalized JSON schema containing path hierarchy (`scanned_root`, `relative_path`, `file_path`, `file_name`), file size, temporal facets (`year`, `month`, `day`, `timestamp`), camera data, exposure settings, dimensions, video attributes, and extraction status.
- **FR-012**: System MUST isolate per-file parsing errors, ensuring corrupt headers or unsupported format sub-types generate structured diagnostic records with error causes without crashing the extraction pipeline.

### Key Entities *(include if feature involves data)*

- **MediaMetadataDocument**: Complete normalized document representing a single processed media asset.
  - `id`: Unique identifier (UUID or deterministic hash).
  - `file_context`: File location data (`scanned_root`, `file_path`, `relative_path`, `file_name`, `file_size_bytes`, `extension`, `media_category`).
  - `temporal`: Normalized date information (`iso_timestamp`, `year`, `month`, `day`, `hour`, `minute`, `second`, `source`).
  - `camera`: Hardware metadata (`make`, `model`, `lens_model`, `software`).
  - `exposure`: Photographic parameters (`focal_length_mm`, `focal_length_35mm`, `aperture_f_number`, `exposure_time_seconds`, `iso_equivalent`, `flash_fired`).
  - `location`: Geolocation data (`latitude`, `longitude`, `altitude_meters`).
  - `dimensions`: Visual geometry (`pixel_width`, `pixel_height`, `orientation_tag`).
  - `video_properties`: Video-specific attributes (`duration_ms`, `video_codec`, `audio_codec`, `frame_rate_fps`, `rotation_degrees`).
  - `extraction_status`: Operational diagnostics (`status`, `warnings`, `errors`).

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Successfully extracts metadata from 100% of standard photo and video formats with valid headers, with zero unhandled panics or process crashes.
- **SC-002**: Embedded capture timestamps are accurately converted into calendar facets (`year`, `month`, `day`) for at least 99% of media files containing valid date tags.
- **SC-003**: 100% of emitted JSON records conform to the target schema and parse as valid JSON ready for document database insertion (e.g. MongoDB).
- **SC-004**: Metadata extraction achieves an average throughput of at least 250 files per second per CPU core for compressed photos and container headers by avoiding full image decompression.
- **SC-005**: Video container headers on files exceeding 2 GB are parsed in under 50 milliseconds per file using streaming atom readers.
- **SC-006**: 100% of corrupted, unreadable, or malformed media headers produce a valid JSON record containing basic file attributes and detailed diagnostic error causes.

## Assumptions

- Input files are identified media assets provided as accessible file paths (e.g. from the directory scanner or manual input).
- Metadata extraction is header-based; full visual raster decoding or frame rendering is deferred to preserve high throughput.
- When embedded metadata lacks capture dates, file system modification timestamps are used as a fallback and explicitly flagged as `filesystem_fallback`.
- Output records are emitted as streaming newline-delimited JSON (NDJSON) or serialized JSON objects suitable for direct ingestion into document databases.
