# Feature Specification: Media Directory Walker

**Feature Branch**: `001-media-directory-walker`

**Created**: 2026-11-07

**Status**: Draft

**Input**: User description: "High-throughput asynchronous directory walker for media discovery with backpressured batch streaming"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Recursive Media Discovery on Target Directories & External Drives (Priority: P1)

As a media library operator, I want to supply one or more specific directory paths—typically pointing to an external hard disk or mounted storage volume (such as `/Volumes/SP-1`), an SD card backup, or local directory hierarchies—and have the system recursively scan all subdirectories to identify photo and video files, so that I have an accurate, structured inventory of media ready for metadata extraction and deduplication.

**Why this priority**: This is the core entry point and primary value driver for the pipeline. Real-world archives and photo dumps usually reside on external storage media (e.g., `/Volumes/SP-1`). Without reliable directory scanning and file discovery on these mounted paths, no downstream extraction or indexing can take place.

**Independent Test**: Can be fully tested by pointing the scanner at a fixture folder or external drive mount path containing known quantities of image files, video files, and non-media files. The test succeeds when all media files are discovered with correct paths and file sizes, while non-media files are ignored.

**Acceptance Scenarios**:

1. **Given** an external hard disk or storage volume mounted at a designated path (e.g., `/Volumes/SP-1`) containing nested photo and video directories, **When** traversal is executed for `/Volumes/SP-1`, **Then** all media files across all subdirectories on the external drive are discovered with canonical paths, byte sizes, and modification timestamps recorded.
2. **Given** a target directory containing non-media assets (e.g., `.txt`, `.pdf`, `.DS_Store`, system volume information), **When** traversal is executed, **Then** non-media files are ignored and excluded from discovery results.
3. **Given** media files with varied case extensions (e.g., `.JPG`, `.jpeg`, `.MOV`, `.mp4`, `.CR3`), **When** traversal is executed on the target drive, **Then** all variations are recognized correctly regardless of case.

---

### User Story 2 - Backpressured Batch Streaming to Downstream Consumers (Priority: P2)

As a pipeline architect, I want discovered files to be emitted progressively in bounded batches with flow control, so that downstream processing starts immediately without exhausting system memory on multi-million asset collections.

**Why this priority**: Scanning millions of files in large archives cannot buffer all paths in memory before passing them to extractors. Streaming with backpressure ensures flat memory consumption and eliminates startup latency.

**Independent Test**: Can be tested by running a scan over 50,000 files with a simulated throttled consumer. The test succeeds when the first batch arrives within seconds, and the scanner pauses traversal when downstream buffers reach maximum capacity.

**Acceptance Scenarios**:

1. **Given** a large directory containing thousands of media files, **When** the scan starts, **Then** the first batch of discovered items is delivered to downstream consumers within 2 seconds.
2. **Given** a downstream consumer that is temporarily paused or processing slowly, **When** internal delivery queues reach their configured high-water mark, **Then** the scanner halts further file reading until the consumer draws down the queue.
3. **Given** the consumer resumes pulling batches, **When** queue levels drop below the threshold, **Then** traversal resumes automatically without losing progress or dropping files.

---

### User Story 3 - Resilient Traversal & Live Telemetry (Priority: P3)

As a systems operator running unattended scans on massive storage volumes, I want the scanner to bypass inaccessible directories and broken links gracefully while providing real-time telemetry, so that I have clear operational visibility and scans never terminate prematurely.

**Why this priority**: Real-world file archives inevitably have permission restrictions, corrupted entries, and deep structures. The scanner must be resilient and self-reporting to support reliable background operations.

**Independent Test**: Can be tested by pointing the scanner at a directory tree containing unreadable folders, circular directory links, and deeply nested paths. The test succeeds when the scan completes without crashing, logs skipped paths with explicit error causes, and emits periodic throughput statistics.

**Acceptance Scenarios**:

1. **Given** a subdirectory with restricted read permissions, **When** the scanner encounters it, **Then** the scanner records a permission error in the skip log and continues scanning all other directories.
2. **Given** a circular symbolic link within the directory tree, **When** encountered during traversal, **Then** the scanner detects the cycle, logs the loop event, and avoids redundant or infinite traversal.
3. **Given** an active scanning run, **When** traversal is underway, **Then** the system emits real-time metrics (directories visited, media files found, current items per second, and error count) at regular intervals.

---

### Edge Cases

- **External Drive Disconnection / Unmount**: If an external hard disk (e.g., `/Volumes/SP-1`) is unmounted or disconnected mid-traversal, the system must detect the I/O failure, isolate the error without process crashing, and record the session interruption with appropriate diagnostics.
- **External Drive Read Latency**: Traversal across slower external USB 2.0/3.0 drives or spinning disks must not starve memory or block async runtimes; backpressure and work-stealing threads must dynamically adapt to physical I/O throughput.
- **File Disappearance**: A file discovered during directory reading is deleted or moved before its metadata is read. The scanner must capture the change as a transient skip without crashing.
- **Deep Nesting**: Directory hierarchies exceeding 100 nested folders must be traversed without hitting stack overflow limits.
- **Special Characters and Non-UTF-8 Paths**: Path names containing emoji, accented characters, or non-standard byte sequences must be preserved accurately or cleanly handled with diagnostic logging.
- **Zero-Byte Media Files**: Files with media extensions that have a file size of 0 bytes must be flagged as empty items and routed appropriately.
- **Empty Directories**: Thousands of empty directories must be traversed rapidly without emitting empty batches to consumers.
- **Early Scan Cancellation**: If an operator requests cancellation, the scanner must halt traversal promptly, release file descriptors, and cleanly flush any partially filled batches.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST accept one or more specified directory paths—including external hard disk and storage volume mount paths (e.g., `/Volumes/SP-1`, `/mnt/media`), local directory trees, or removable storage—as root scan targets.
- **FR-002**: System MUST recursively traverse all accessible subdirectories beneath each root path.
- **FR-003**: System MUST identify media files using a configurable list of supported photo and video file extensions, performing case-insensitive matching.
- **FR-004**: System MUST capture core file attributes for each discovered media asset, including canonical path, file size in bytes, and last modification timestamp.
- **FR-005**: System MUST group discovered media items into configurable batch sizes and stream them progressively to consumers.
- **FR-006**: System MUST enforce backpressure mechanisms that regulate traversal speed according to downstream consumption rates.
- **FR-007**: System MUST detect and skip circular directory references and symbolic links that loop back to already-visited paths.
- **FR-008**: System MUST isolate and record file system access errors (e.g., permission denied, broken links, path not found) in an error registry without terminating the scan session.
- **FR-009**: System MUST emit operational telemetry metrics at regular intervals, reporting directories scanned, media files discovered, items-per-second throughput, and buffer queue depth.
- **FR-010**: System MUST support graceful cancellation, stopping new directory reads and safely closing batch streams upon request.

### Key Entities *(include if feature involves data)*

- **Discovered Media Item**: Represents a single identified media file on disk. Attributes include canonical path, file size in bytes, last modified timestamp, media classification (photo vs. video), and detected file extension.
- **Discovery Batch**: An ordered collection of discovered media items packaged for downstream processing. Attributes include batch sequence number, item count, and creation timestamp.
- **Scan Session**: Represents an end-to-end directory traversal execution. Attributes include session identifier, root paths, start time, completion time, status (active, completed, cancelled, failed), total directories visited, total media files found, and error summary.
- **Scan Error Record**: A diagnostic entry for an unreadable or skipped path. Attributes include target path, error category (permission denied, circular link, vanished path, I/O error), timestamp, and diagnostic details.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Discovers 100% of accessible media files in target directories, with zero false positives from non-media files.
- **SC-002**: First batch of discovered media items is available to consumers within 2 seconds of scan initiation on local storage.
- **SC-003**: Memory usage of the directory traversal component remains bounded under 100 MB regardless of total collection size (validated against collections exceeding 1,000,000 files).
- **SC-004**: Achieves sustained traversal and file classification throughput of at least 15,000 items per second on high-performance local storage.
- **SC-005**: Zero unhandled panics or abnormal process terminations when encountering inaccessible folders, circular links, or vanished files.
- **SC-006**: 100% of access failures and skipped paths are captured with human-readable error reasons in the session summary.

## Assumptions

- Specified root target paths and external storage volumes (e.g., `/Volumes/SP-1`, external hard drives, or mounted network folders) are mounted and accessible through the operating system's standard file system interface prior to initiating traversal.
- Initial media classification is determined by file extension and basic file attribute evaluation; deep content parsing and corrupt payload verification occur in downstream extraction phases.
- Downstream consumers process items asynchronously and communicate backpressure capacity through bounded channel buffers.
- Hidden files and system directories (e.g., `.git`, `.thumbnails`, `.Trash`, `.DS_Store`) are skipped by default unless explicitly configured otherwise.
- The system runs under standard user permissions; directories inaccessible under those permissions will be recorded as skipped items rather than escalating privileges.
