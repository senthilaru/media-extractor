# Feature Specification: Incremental Media Scan & Database Change Caching

**Feature Branch**: `008-incremental-media-scan`

**Created**: 2026-11-09

**Status**: Draft

**Input**: User description: "what will happen if i execute for the same path again and again, eg. /spsa/temp run for more than 1 times,I dont want to calculate again, if its found in the DB, to save more time"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Incremental Re-Scan Skipping Unmodified Files (Priority: P1) 🎯 MVP

When operators repeatedly scan a large media directory (such as a 1 TB dataset with tens of thousands of media files), the system must query the existing database index to detect files that have already been extracted and have not changed on disk (matching file path, size, and modification timestamp). Unmodified files must be bypassed immediately without re-reading file payloads, re-parsing container headers, or re-computing cryptographic hashes, completing subsequent scans in seconds instead of hours.

**Why this priority**: Directly solves the user's core problem ("I don't want to calculate again, if it's found in the DB, to save more time"). Re-running scans over existing media libraries should be near-instantaneous for unchanged files.

**Independent Test**: Can be tested by executing a scan over a media directory into a database, then immediately re-running the exact same scan command, verifying that all previously extracted files are identified as cached/unchanged, extraction work is skipped, and total scan duration drops from minutes/hours to seconds.

**Acceptance Scenarios**:

1. **Given** a directory containing media files previously scanned and persisted in the database, **When** the operator re-runs the scan command against the same directory, **Then** the system checks existing database records and identifies all unchanged files as cached.
2. **Given** an unchanged media file detected in the database, **When** the extraction stage executes, **Then** the system skips container parsing, EXIF extraction, and fingerprint calculation for that file.
3. **Given** an active incremental scan, **When** live progress is displayed on the terminal, **Then** the progress indicator visibly displays the count and rate of skipped/cached files alongside newly processed items.
4. **Given** an incremental scan completes, **When** the execution summary report is printed, **Then** it clearly reports the breakdown of items: total discovered, skipped (already indexed & unchanged), newly extracted, and updated records.

---

### User Story 2 - Detection and Re-Extraction of New or Modified Files (Priority: P1)

When a previously scanned directory has new media files added or existing media files modified (e.g. edited photo, re-encoded video, or altered file size/timestamp), the system must selectively process only the new and modified files while continuing to bypass unchanged assets.

**Why this priority**: Media libraries are dynamic; users periodically add new photos/videos or edit metadata tags. The system must maintain data freshness without forcing full re-scans of the entire archive.

**Independent Test**: Can be tested by modifying the timestamp/size of one file and adding a new file to a previously indexed directory, re-running the scan, and verifying that only the modified and new files undergo extraction while all other files are skipped.

**Acceptance Scenarios**:

1. **Given** a newly added media file in a previously scanned directory, **When** an incremental scan runs, **Then** the system detects that no matching record exists in the database and fully extracts and persists the new file.
2. **Given** an existing media file whose filesystem modification timestamp or file size has changed since the last scan, **When** an incremental scan runs, **Then** the system recognizes the file as modified, re-extracts its metadata, and updates the database record.
3. **Given** an incremental scan encountering a mix of unchanged, modified, and new files, **When** the scan finishes, **Then** the database accurately reflects the latest state of all assets with appropriate update counts.

---

### User Story 3 - Forced Re-Index & Cache Invalidation Control (Priority: P2)

Operators and administrators require the ability to bypass incremental skipping on demand (e.g., via a `--force` or `--reindex` flag) when extraction algorithms improve, new metadata fields are introduced, or a complete database refresh is desired.

**Why this priority**: Operators occasionally need to re-extract all metadata (for example, after updating extraction parsers or switching from fast mode to full deep hashing) without manually wiping the database.

**Independent Test**: Can be tested by invoking the scanner on an already-indexed directory with the force flag, verifying that all files undergo full re-extraction regardless of existing database records.

**Acceptance Scenarios**:

1. **Given** an existing indexed media directory in the database, **When** the operator specifies a force re-index option (such as `--force` or `--reindex`), **Then** the system ignores pre-existing records and re-extracts all discovered media items.
2. **Given** a scan executed with the force option, **When** the operation finishes, **Then** all pre-existing records are updated with freshly extracted metadata, and the execution summary reflects zero skipped files.

---

### User Story 4 - High-Throughput Batch Pre-Filtering (Priority: P2)

To ensure incremental checks do not become a database latency bottleneck on million-asset archives, pre-filtering checks must query the database in bulk batches rather than issuing single-file network roundtrips.

**Why this priority**: Querying the database one file at a time for 50,000 files would introduce tens of seconds or minutes of network latency. Batch querying ensures pre-filtering keeps pace with high-speed filesystem discovery.

**Independent Test**: Can be tested on a dataset of 10,000 files, verifying that database pre-filtering queries are batched (e.g., checking 500 file paths per query) and discovery pre-filtering achieves throughput exceeding 1,000 items per second.

**Acceptance Scenarios**:

1. **Given** a batch of discovered media paths from the directory scanner, **When** the pre-filter evaluates existing records, **Then** it issues a single batched query projecting only minimal change-detection fields (path, size, modification timestamp).
2. **Given** a storage sink with indexed query paths, **When** evaluating pre-filter batches, **Then** database lookup latency averages less than 20 milliseconds per batch.
3. **Given** a disconnected or unreachable storage sink during pre-filtering, **When** the check executes, **Then** the system reports a clear storage connectivity error and halts safely per configured fail-fast policies.

---

### Edge Cases

- **File renamed or moved to another directory**: File path changes; system treats the new path as a new file (extracts metadata) and persists it. (Historical duplicate detection handles content deduplication).
- **File truncated or changed with same timestamp**: File size difference triggers re-extraction even if the modification timestamp was preserved.
- **Clock skew or sub-second timestamp precision**: Filesystem timestamps compared against stored database ISO timestamps allow standard sub-second boundary tolerance to prevent false re-extraction loops.
- **Empty or brand-new database**: Pre-filtering finds zero matching records; all items proceed directly to extraction with negligible query overhead.
- **Database read failure during pre-filtering**: System fails fast with an informative error rather than silently re-extracting everything or crashing unexpectedly.
- **Single file scan mode**: Pre-filtering applies to single file extraction as well; if the single target file is already indexed and unchanged, it reports status or emits existing cached metadata without re-reading the file unless `--force` is supplied.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST check the configured storage sink to determine if discovered media files have already been indexed before dispatching them to metadata extraction.
- **FR-002**: The system MUST compare filesystem attributes (`file_path`, `file_size_bytes`, and `modified_at` timestamp) against stored database values to determine whether a media file is unmodified.
- **FR-003**: The system MUST bypass container parsing, EXIF extraction, and cryptographic hashing for any media file determined to be unmodified in the database.
- **FR-004**: The system MUST re-extract and update metadata for any previously indexed file whose size or modification timestamp differs from the stored record.
- **FR-005**: The system MUST execute database pre-filtering in bulk batches (matching scanner batch sizes) using projected attribute queries to prevent per-file network roundtrip latency.
- **FR-006**: The system MUST provide an explicit CLI option (e.g., `--force` or `--reindex`) to override incremental skipping and force full re-extraction of all media assets.
- **FR-007**: The system MUST track and display real-time counts and rates of skipped/cached files in live terminal telemetry during active scanning.
- **FR-008**: The system MUST include incremental metrics (items skipped/cached, newly extracted, and updated) in the final execution summary report.
- **FR-009**: The system MUST default to incremental scanning whenever an authenticated storage sink is configured, unless force re-index is explicitly requested.
- **FR-010**: The system MUST ensure database index coverage on `file_context.file_path` to support high-speed batch lookup performance.

### Key Entities *(include if feature involves data)*

- **MediaChangeDescriptor**: Represents the minimal filesystem attributes needed to detect modifications (`file_path`, `file_size_bytes`, `modified_at`).
- **BatchPreFilterResult**: Encapsulates the partitioned result of a batch evaluation:
  - `unmodified_items`: Discovered items that exist in the database with matching size and timestamp (to be skipped).
  - `modified_items`: Existing items with altered size or timestamp (to be re-extracted and updated).
  - `new_items`: Items not found in the database (to be extracted and newly inserted).
- **IncrementalScanSummary**: Part of the final execution summary tracking:
  - `total_discovered`: Overall media items found on disk.
  - `skipped_unchanged`: Items verified as already indexed and unmodified.
  - `newly_extracted`: New items extracted for the first time.
  - `re_extracted_modified`: Existing items that were re-extracted due to filesystem changes.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: When re-running a scan on an unchanged 1 TB media dataset (or 10,000+ files) already stored in the database, total execution time decreases by at least 95% compared to the initial scan (completing in under 30 seconds on local SSD/network storage).
- **SC-002**: Database batch pre-filtering evaluates at least 2,000 items per second when connected to a local or high-speed storage sink.
- **SC-003**: Zero unnecessary disk read I/O operations occur for media files identified as unmodified during an incremental scan.
- **SC-004**: 100% of newly added or modified files in an indexed directory are accurately detected, extracted, and updated during an incremental scan.
- **SC-005**: The terminal progress display and final execution summary explicitly report the exact counts of skipped, new, and modified items within 500 milliseconds of scan completion.

## Assumptions

- A persistent storage sink (such as MongoDB) is configured and reachable for incremental scanning; if no storage sink is configured (e.g. streaming solely to stdout), incremental caching is inactive unless an external index is supplied.
- Filesystem modification timestamps (`mtime`) and byte sizes reported by the operating system are reliable indicators of file content changes.
- Stored records in the database contain the canonical absolute file path, file size in bytes, and original filesystem modification timestamp.
- Operators prefer incremental skipping by default on re-scans to save execution time and hardware resources.
