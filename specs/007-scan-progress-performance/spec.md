# Feature Specification: Scan Progress Telemetry & High-Throughput Performance Optimization

**Feature Branch**: `007-scan-progress-performance`

**Created**: 2026-11-08

**Status**: Draft

**Input**: User description: "when I run with sample path ./target/release/metadata-extractor /spsa/temp this path has 1 TB of data (photos and videos) its takes more than 40 mins, still it extracted only 13K files, and it has no clue whats going on, how long will it take, is there any other way to improve speed and show some high level progress"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Real-Time Progress, High-Level Metrics & ETA Visibility (Priority: P1)

Operators scanning large media collections (such as a 1 TB dataset with tens of thousands of photos and videos) require continuous, real-time visual progress reporting on the terminal. The display must show items discovered, items processed, data volume (MB/GB processed and observed), live throughput rates (items/sec and MB/sec), elapsed execution time, and an accurate Estimated Time to Completion (ETA), eliminating operator uncertainty during long-running tasks.

**Why this priority**: Directly addresses the primary user frustration ("has no clue whats going on, how long will it take, show some high level progress"). Operators must have immediate, actionable visibility into system activity, current throughput, and expected completion without guessing whether the process is hung.

**Independent Test**: Can be tested by invoking the application on a multi-gigabyte directory containing mixed media assets and verifying that standard error continuously updates with live progress statistics, elapsed time, throughput rates, and ETA, culminating in a clear final completion summary.

**Acceptance Scenarios**:

1. **Given** an ongoing scan of a media directory, **When** the extraction process runs on an interactive terminal, **Then** standard error displays a live, dynamically updated progress status showing: items discovered, items extracted, data volume processed, current throughput (items/sec and MB/sec), elapsed time, and estimated time remaining (ETA).
2. **Given** an active scan in progress, **When** the system transitions between operational phases (such as directory discovery, metadata extraction, content hashing, and database persistence), **Then** the progress indicator visibly displays the active operational phase.
3. **Given** an active extraction session, **When** standard output is redirected to a file or piped to another tool, **Then** the progress indicator writes strictly to standard error, ensuring standard output data streams remain completely clean and uncorrupted.
4. **Given** a completed or interrupted scan, **When** the operation finishes, **Then** the system outputs a comprehensive execution summary report displaying total elapsed time, total media items discovered and processed, total byte volume processed, average throughput, and counts of successes, warnings, and errors.

---

### User Story 2 - High-Throughput Pipelined Execution & Concurrency Optimization (Priority: P1)

Operators processing large-scale media archives need the extraction engine to maximize system throughput and hardware core utilization by executing directory discovery, metadata extraction, and storage ingestion concurrently through an asynchronous pipelined architecture with bounded memory backpressure.

**Why this priority**: In the user's workload, processing 13,000 files required over 40 minutes (~5.4 files/sec), demonstrating severe throughput bottlenecks and idle hardware cycles caused by synchronous batch serialization. Decoupled pipelining ensures continuous execution across all available hardware resources.

**Independent Test**: Can be tested by comparing execution elapsed time and CPU utilization on a test dataset, verifying that directory traversal continues discovering files while parallel extraction workers concurrently process previously discovered batches without blocking.

**Acceptance Scenarios**:

1. **Given** a directory tree with tens of thousands of media files, **When** scanning begins, **Then** directory traversal and metadata extraction run concurrently across available processor cores so that slow individual files do not block ongoing discovery.
2. **Given** a multi-core host system, **When** metadata extraction processes media batches, **Then** compute workloads are dynamically distributed across worker threads, maximizing CPU utilization without thread thrashing.
3. **Given** a fast storage scanner producing items faster than extraction or database sinks can consume, **When** internal queues reach capacity, **Then** bounded backpressure pauses discovery without exhausting system memory.
4. **Given** continuous execution on a 1 TB dataset, **When** memory usage is monitored over time, **Then** resident memory remains strictly bounded within operational limits (< 500 MB RAM) without linear growth.

---

### User Story 3 - Configurable Extraction & Hashing Modes for Terabyte-Scale Media (Priority: P2)

Operators scanning terabyte-scale media archives need configurable execution modes (such as a fast metadata-only mode, or file size thresholds for deep content hashing) so that large multi-gigabyte video files do not bottleneck the entire scan when full bitwise deduplication hashes are not immediately required.

**Why this priority**: Full cryptographic hashing (e.g., reading 100% of bytes for Blake3 digests) and perceptual visual hashing on multi-gigabyte video files saturate storage I/O bandwidth. Allowing operators to choose between fast metadata extraction and deep content hashing provides a 10x-50x speed improvement for initial cataloging.

**Independent Test**: Can be tested by running extraction on a sample folder containing large video files with fast mode enabled, verifying that metadata (dimensions, codecs, timestamps) is extracted in a fraction of the time without reading the entirety of large video payloads.

**Acceptance Scenarios**:

1. **Given** an operator running a scan with fast extraction enabled, **When** processing photos and videos, **Then** the system extracts file metadata and container tags without reading full-file bitwise payloads for deep cryptographic or perceptual hashes, achieving high processing speed (> 50 files/sec on standard SSD).
2. **Given** an operator running in full extraction mode, **When** media files are processed, **Then** complete cryptographic fingerprints and perceptual deduplication hashes are generated as configured, accompanied by visible progress and throughput telemetry.
3. **Given** a configurable file size threshold for deep hashing (e.g., files larger than 100 MB), **When** a multi-gigabyte video file is encountered, **Then** the system extracts video stream metadata while skipping or deferring exhaustive bitwise hashing, preventing I/O starvation.
4. **Given** CLI arguments or configuration profiles, **When** the operator specifies `--fast` or selects the extraction depth, **Then** the system clearly displays the active performance mode in the startup telemetry.

---

### User Story 4 - Environment-Adaptive Progress Display & Machine-Readable Output (Priority: P3)

Operators and automated supervisors executing the tool across varied environments (interactive terminal, CI/CD runners, background scripts, or piped workflows) need progress reporting to automatically adapt its rendering style—using interactive multi-metric terminal displays when run interactively, discrete periodic log lines in non-interactive pipes, and structured JSON telemetry when requested.

**Why this priority**: Terminal control characters (carriage returns, ANSI escapes) corrupt text logs and automated pipeline outputs when redirected. Adapting to the environment guarantees professional user experience and clean integration.

**Independent Test**: Can be tested by running the tool interactively, running with stderr redirected to a file, and running with `--quiet`, verifying that each environment receives the appropriate formatting without terminal escape artifact pollution.

**Acceptance Scenarios**:

1. **Given** an interactive terminal (TTY), **When** scanning runs, **Then** the progress indicator dynamically updates in place without scrolling or creating terminal line spam.
2. **Given** standard error redirected to a file or non-interactive pipe, **When** scanning runs, **Then** the system emits discrete, periodic single-line progress updates (e.g., every 5 seconds) without carriage returns or ANSI escape codes.
3. **Given** the `--quiet` or `-q` flag is supplied, **When** scanning runs, **Then** all routine progress telemetry is suppressed while fatal errors and exit codes remain intact.
4. **Given** an automated monitoring supervisor requesting structured metrics, **When** structured output is configured, **Then** progress events are emitted as discrete machine-readable JSON lines on standard error.

---

### Edge Cases

- **Zero-item or empty directory**: System completes gracefully within milliseconds, displaying a summary with 0 items discovered and 0 seconds elapsed without encountering division-by-zero errors in rate or ETA calculation.
- **Single massive media file (e.g., 50 GB video)**: Progress telemetry indicates active processing on the file rather than appearing frozen, and overall ETA calculations adjust smoothly without wild oscillations.
- **Permission-denied directories or corrupt media headers**: Progress counters increment skip and error tallies in real time; the overall pipeline continues without aborting or hanging.
- **Unknown total item count during traversal**: During initial directory traversal before the tree is fully enumerated, the display shows observed items and throughput rate, transitioning to a definitive percentage and ETA as the discovery baseline stabilizes.
- **Terminal window resizing or narrow widths**: The progress display formats dynamically to fit the terminal width, truncating or wrapping cleanly without generating multi-line visual artifacts.
- **Early operator interruption (SIGINT / Ctrl+C)**: The system catches the interrupt signal, halts worker threads cleanly, and displays a partial execution summary of work completed prior to termination.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST render real-time execution progress to standard error, including counts of items discovered, items processed, current processing rate (in items per second), and total elapsed time.
- **FR-002**: The system MUST calculate and display an Estimated Time to Completion (ETA) based on moving-average processing throughput and remaining observed items or byte volume.
- **FR-003**: The system MUST compute and display data volume metrics (processed data volume in MB/GB and I/O throughput rate in MB/s) to provide accurate physical transfer visibility when processing multi-gigabyte files.
- **FR-004**: The system MUST visibly display the active operational phase (e.g., Directory Traversal, Metadata Extraction, Content Fingerprinting, Storage Ingestion).
- **FR-005**: The system MUST print a comprehensive final execution summary upon scan completion or termination, reporting total duration, total files processed, total byte volume, average throughput rate, and categorized success/failure counts.
- **FR-006**: The system MUST decouple directory scanning, metadata extraction, and storage writes into an asynchronous, pipelined architecture with bounded channels to prevent synchronous stalling.
- **FR-007**: The system MUST dynamically utilize available multi-core worker threads for metadata extraction while enforcing backpressure limits to keep resident memory bounded.
- **FR-008**: The system MUST provide a configurable fast extraction performance mode (e.g., `--fast` or metadata-only profile) that skips full-file bitwise cryptographic and perceptual hashing while preserving rich container and EXIF metadata extraction.
- **FR-009**: The system MUST support a configurable file size threshold above which exhaustive cryptographic hashing is skipped or deferred for large media files to protect I/O throughput.
- **FR-010**: The system MUST automatically detect interactive (TTY) versus non-interactive environments, rendering in-place progress updates on interactive terminals and periodic discrete lines on non-interactive streams.
- **FR-011**: The system MUST isolate all progress and telemetry output to standard error, ensuring standard output data streams (e.g., JSON records) remain strictly valid and unpolluted.
- **FR-012**: The system MUST respect a quiet flag (`-q` / `--quiet`) that suppresses all progress indicators while preserving fatal error reporting.

### Key Entities *(include if feature involves data)*

- **ScanProgressSnapshot**: Represents an instantaneous point-in-time progress state containing elapsed duration, items discovered, items processed, byte volume processed, current throughput rate (items/sec and MB/sec), active operational phase, estimated remaining time (ETA), and error/skip counts.
- **ExecutionSummary**: Represents the final post-scan audit report containing total elapsed time, total media items discovered and processed, total byte volume handled, overall throughput rates, and categorized counts of successes, warnings, and errors.
- **ExtractionPerformanceProfile**: Represents user-specified runtime execution settings, including performance mode (Fast vs Full), hashing file size thresholds, worker concurrency bounds, and telemetry update intervals.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Live progress telemetry updates smoothly at a steady interval (between 1 and 4 times per second) with less than 1% CPU overhead dedicated to telemetry tracking.
- **SC-002**: Moving-average throughput (items/sec, MB/sec) and Estimated Time to Completion (ETA) begin displaying accurately within 5 seconds of scan initiation.
- **SC-003**: When scanning a 1 TB media dataset in fast extraction mode, processing throughput increases by at least 10x compared to full-file bitwise hashing mode (achieving at least 50+ media files per second on standard SSD storage).
- **SC-004**: Standard output redirected to a file or pipe contains 100% valid, uncorrupted output data with zero escape sequences or telemetry lines leaking into the data stream.
- **SC-005**: Resident memory consumption during continuous processing of collections with over 100,000 media files or 1 TB of media assets remains strictly bounded under 500 MB RAM.
- **SC-006**: Upon scan completion or user cancellation (Ctrl+C), the complete execution summary report is displayed on standard error within 500 milliseconds.

## Assumptions

- The target runtime environment includes both interactive terminal emulators supporting ANSI cursor controls and non-interactive scripted environments (pipes, redirects, CI).
- The 1 TB dataset comprises a representative mixture of photos (JPEG, HEIC, RAW) and video files (MP4, MOV, MKV), with video files accounting for a majority of the byte volume.
- Fast metadata extraction extracts container headers, EXIF/QuickTime tags, dimensions, durations, and timestamps without reading the entire video stream payload.
- High-level progress can compute moving-average ETA based on observed files and data volume, updating dynamically as directory discovery progresses.
- Standard hardware includes a modern multi-core processor (such as Apple Silicon M1 or x86_64 equivalent) and local or high-speed attached storage.
