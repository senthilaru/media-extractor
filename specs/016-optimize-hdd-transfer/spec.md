# Feature Specification: External HDD I/O Transfer & Buffer Optimization

**Feature Branch**: `016-optimize-hdd-transfer`

**Created**: 2026-11-13

**Status**: Draft

**Input**: User description: "Tune the I/O buffer size for efficient data transfer b/n external HDDs. Is writing or reading from external HDD strictly sequential? I think parallelism will degrade here for HDDs right? Find optimized effcient way to deal copying with external HDDs"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Tuned Chunk Sequential Streaming for External Hard Drives (Priority: P1)

When migrating and copying gigabytes of 4K video clips and high-resolution photos between external USB hard disk drives, the operator experiences prolonged transfer durations when file data is transferred in tiny or suboptimal memory chunks. The operator needs file copy operations between external mechanical drives to use an optimized contiguous buffer size (such as 4 MB to 8 MB) and strictly sequential file streaming per physical drive, enabling the drive heads to read and write contiguous sectors at maximum sustained platter bandwidth without thrashing.

**Why this priority**: Mechanical HDDs are severely bottlenecked by head seek latency (8–15 ms per random seek). Operating at contiguous sequential read/write speeds of 120–180 MB/s versus thrashing at 15–30 MB/s determines whether a multi-gigabyte library ingestion takes minutes or hours. This forms the foundational performance tier for all drive-to-drive transfers.

**Independent Test**: Ingest or replicate a set of large video files (e.g. 10 GB total) between two external USB HDDs and verify that transfer throughput operates at the sustained physical sequential speed of the drive hardware without buffer starvation or excessive context switching.

**Acceptance Scenarios**:

1. **Given** two mounted external HDDs with active transfer tasks, **When** file data is streamed from source to destination, **Then** data is transferred in contiguous 8 MB chunks rather than default 1 MB chunks, maximizing continuous sustained sequential throughput.
2. **Given** multiple files queued for ingestion or replication on the same physical HDD, **When** transfer begins, **Then** files are transferred sequentially one file at a time per drive spindle, preventing concurrent head movements across different disk sectors.

---

### User Story 2 - In-Flight Streaming Integrity Verification (Priority: P2)

Currently, verifying data integrity requires reading the source file, writing the target file, and then reopening the target file to perform a secondary full-disk read to compute its cryptographic hash. On mechanical hard drives, this secondary read pass doubles the required disk read volume (e.g., turning a 60 GB copy into 120 GB of disk reads), keeping drive actuators busy twice as long. The operator needs cryptographic hash calculation to occur in-flight in memory while the stream is written, verifying integrity immediately upon stream completion and eliminating the costly second read pass.

**Why this priority**: Eliminating redundant full-disk re-reads cuts the total physical I/O requirement on destination mechanical HDDs in half, drastically reducing sync duration while maintaining 100% cryptographic data integrity.

**Independent Test**: Copy a verified media file to an external HDD with hash verification enabled, and verify that the target cryptographic hash is verified against the source digest without performing a second sequential read from the target disk platter.

**Acceptance Scenarios**:

1. **Given** a source file with a known cryptographic hash, **When** streaming data to the destination drive, **Then** the stream hasher computes the cryptographic hash continuously as bytes pass through the memory buffer.
2. **Given** completed streaming transfer and immediate OS flush (`fsync`), **When** validating integrity, **Then** the in-flight computed hash is compared directly against the expected source hash, completing verification without a separate full-file disk re-read.
3. **Given** an integrity mismatch detected during in-flight streaming, **When** transfer concludes, **Then** the partially copied target file is rolled back and an error is logged.

---

### User Story 3 - Per-Drive Serialization with Inter-Drive Concurrency (Priority: P3)

When syncing primary master assets to multiple replica drives or transferring files between independent drive pairs, parallel read/write streams targeting the *same* physical hard drive degrade performance due to head thrashing. However, transfers between *different* physical drives (or fan-out to distinct physical replicas on separate USB channels) can operate in parallel without head contention. The operator needs a concurrency model that strictly serializes operations per physical drive spindle while allowing non-competing operations across separate physical drives.

**Why this priority**: Ensures optimal hardware utilization across multi-drive configurations, preventing accidental spindle contention while accelerating multi-target replica fanout.

**Independent Test**: Trigger a replication sync targeting multiple external drives and verify that operations on any single physical drive remain strictly sequential, while independent drives receive data without cross-drive blocking.

**Acceptance Scenarios**:

1. **Given** multiple files queued for a single physical HDD, **When** copying commences, **Then** only one active read or write operation is permitted on that specific drive spindle at any instant.
2. **Given** two separate destination replica HDDs connected to different USB controllers, **When** synchronizing assets from primary master, **Then** data can be dispatched without interleaving multiple access streams on any single physical disk platter.

---

### User Story 4 - Configurable I/O Buffer Tuning & Performance Overrides (Priority: P4)

Different storage technologies exhibit distinct performance sweet spots: spinning mechanical HDDs benefit from larger 4 MB–8 MB buffers to preserve sequential sector tracking, while solid-state drives (NVMe/SSD) have zero seek latency and may perform well with standard buffers. The operator needs the ability to customize buffer sizes and concurrency settings via configuration or CLI arguments for specialized hardware setups.

**Why this priority**: Provides flexibility for power users with diverse storage hardware (e.g., hybrid setups combining high-speed NVMe staging drives with cold mechanical archive HDDs).

**Independent Test**: Run `ingest` or `sync-replicas` with custom buffer options (e.g. `--buffer-size 8M` or `--sequential-spindle`) and verify that the transfer engine honors the specified settings.

**Acceptance Scenarios**:

1. **Given** a user running a transfer command with `--buffer-size 8M`, **When** streaming begins, **Then** the file transfer service allocates 8 MB bounded streaming buffers for the operation.
2. **Given** an invalid or excessive buffer size requested (e.g. exceeding safe memory limits), **When** parsing configuration, **Then** the system gracefully constrains the buffer size to safe operational thresholds and notifies the operator.

---

### Edge Cases

- What happens when transferring a file smaller than the buffer size (e.g., a 200 KB image with an 8 MB buffer)?
  - The buffer allocation should be capped at `min(file_size, buffer_capacity)` to avoid allocating unnecessary memory.
- What happens when an external HDD is slow or experiences transient USB bus stalls during an 8 MB write?
  - Streaming writes must support graceful backpressure and time tracking so progress bars and throughput telemetry accurately reflect instantaneous rates.
- What happens if the host system has constrained RAM and multiple transfers are configured?
  - Bounded memory limits must prevent cumulative buffer allocations from exceeding the system's memory envelope.
- What happens if an external drive disconnects or runs out of space mid-transfer?
  - The transfer engine must safely catch I/O errors, cancel active streaming buffers, clean up incomplete target files, and record explicit diagnostic errors.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The file transfer engine MUST support an optimized default streaming buffer size of 8 MB for transfers involving external storage, replacing the previous 1 MB buffer to saturate sequential HDD track throughput.
- **FR-002**: Memory allocations for transfer buffers MUST be bounded to `min(file_size, max_buffer_size)` to prevent wasting memory on small photo or metadata files.
- **FR-003**: All read and write operations targeting the same physical hard disk drive spindle MUST be executed strictly sequentially to prevent read/write head seek thrashing.
- **FR-004**: The system MUST compute cryptographic digests in-flight as data streams through memory buffers, avoiding a second full-disk read pass over the destination file.
- **FR-005**: The system MUST ensure all written data is committed to physical media via storage-level sync (`fsync` / `sync_all`) and write handles are explicitly released before reporting file completion.
- **FR-006**: The transfer engine MUST support CLI parameter `--buffer-size <size>` (with human-readable suffixes like `4M`, `8M`, `16M`) across `ingest` and `sync-replicas` to allow operators to fine-tune I/O sizes for specific storage setups.
- **FR-007**: When synchronizing across multiple independent physical replica drives, the transfer manager MUST allow concurrency across separate physical disks while preserving spindle serialization per individual disk.
- **FR-008**: Real-time throughput metrics (MB/s), transfer progress, and elapsed time MUST update continuously based on actual bytes flushed to destination storage.

### Key Entities

- **StreamingTransferConfig**: Value object defining I/O buffer dimensions (default 8 MB, min 64 KB, max 32 MB), in-flight verification flags, and spindle serialization policies.
- **InFlightStreamHasher**: Streaming cryptographic digest accumulator computing BLAKE3 digests incrementally as byte buffers are written to destination storage.
- **SpindleLock / DriveLockManager**: Synchronization entity preventing concurrent interleaved I/O operations from targeting the same physical drive root.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Sequential transfer throughput between external mechanical HDDs increases by at least 25% compared to baseline 1 MB buffer streaming on multi-gigabyte files.
- **SC-002**: Elimination of the post-transfer disk re-read phase reduces total read I/O on destination drives by 50% for verified file transfers.
- **SC-003**: Zero read/write head thrashing occurs during multi-file transfers on any single external HDD spindle (interleaved concurrent accesses to the same drive are reduced to 0).
- **SC-004**: 100% of transferred files achieve verified cryptographic bitwise parity with source assets upon completion.
- **SC-005**: Memory consumption remains strictly bounded under 64 MB total across all active streaming transfer workers regardless of total dataset size.

## Assumptions

- External HDDs are connected via USB 3.0 / USB-C (UASP-supported) or Thunderbolt interfaces, capable of sustaining sequential throughput between 100 MB/s and 180 MB/s.
- Typical media libraries contain a mix of small assets (photos, 2 MB - 30 MB) and large assets (4K videos, 200 MB - 4 GB), where larger buffer sizes offer the greatest sequential I/O benefits.
- Operating system file caches on macOS manage kernel-level readahead; 8 MB userspace buffers prevent frequent syscall context switching and align with modern external drive controller caches.
- Operators prefer zero data corruption over raw unverified speed, but prioritize avoiding unnecessary redundant disk reads when in-flight memory hashing provides equivalent mathematical certainty.
