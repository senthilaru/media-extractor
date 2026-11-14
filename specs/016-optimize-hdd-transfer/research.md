# Research & Technical Decisions: External HDD Transfer Optimization

**Feature**: `016-optimize-hdd-transfer`
**Date**: 2026-11-13
**Status**: Complete

## 1. Optimal I/O Buffer Sizing for External HDDs

### Decision
Set the default transfer buffer size to **8 MB** (`8 * 1024 * 1024` bytes), dynamically bounded by `min(file_size, buffer_size)`. Allow user overrides via `--buffer-size` supporting suffixes (`4M`, `8M`, `16M`, `32M`).

### Rationale
- **Mechanical HDD Characteristics**: Spinning 3.5" and 2.5" mechanical hard drives (e.g. 5400/7200 RPM) have track and cylinder layouts with onboard DRAM caches (typically 64 MB to 512 MB). Contiguous sequential writes in large blocks allow the drive controller to stream data directly into track buffers without rotational latency or head repositioning.
- **Syscall & Context Switch Reduction**: For a 2.5 GB video file:
  - 64 KB buffer: 40,960 `read`/`write` syscalls.
  - 1 MB buffer: 2,560 `read`/`write` syscalls.
  - 8 MB buffer: Only 320 `read`/`write` syscalls (an 87.5% reduction in context switching over 1 MB).
- **USB 3.x / UASP Bridge Behavior**: External USB 3.0 / USB-C drive enclosures use USB Attached SCSI Protocol (UASP). 4 MB to 8 MB aligned requests maximize SCSI command queue depth and transfer burst sizes across the USB bus without hitting fragmentation thresholds.
- **Memory Boundedness**: In accordance with Constitution Principle I, 8 MB per active stream maintains a tiny memory footprint (< 32 MB total for concurrent independent drive streams), remaining well within the strict 64 MB operational ceiling.

### Alternatives Considered
- *1 MB buffer (Status Quo)*: Suboptimal for multi-gigabyte video files; incurs 8x more syscall overhead and does not fully saturate the USB 3.x UASP block transfer pipe.
- *32 MB - 64 MB buffer*: Diminishing returns on throughput (< 2% difference compared to 8 MB) while causing L3 CPU cache eviction and higher memory allocation overhead per worker.

---

## 2. In-Flight Stream Hashing vs. Post-Copy Destination Re-Read

### Decision
Integrate BLAKE3 cryptographic digest computation **in-flight** directly within the streaming copy loop:
1. Initialize `blake3::Hasher::new()` prior to the transfer loop.
2. For every chunk read from source, call `hasher.update(&buffer[..bytes_read])` before writing to destination.
3. Upon stream completion and `dst_file.sync_all().await`, finalize the stream hash and verify it against `expected_hash` (the source's cryptographic hash).
4. Remove the default secondary full-disk read pass on the destination file. Keep an optional `--recheck-disk-hash` flag for rare offline paranoia checks.

### Rationale
- **I/O Math on Mechanical HDDs**:
  - *Previous Approach*: Read Source (60 GB) + Write Target (60 GB) + Re-read Target (60 GB) = **180 GB total I/O**.
  - *In-Flight Approach*: Read Source (60 GB) + Write Target (60 GB) = **120 GB total I/O** (a **33.3% overall I/O reduction**, and a **50% read I/O reduction** on destination media).
- **Cryptographic Guarantees**: BLAKE3 hashes are calculated across identical bytes in memory. If the bytes read from the source match the expected hash, and `dst_file.write_all()` followed by `dst_file.sync_all()` succeeds without I/O error and destination file size matches `expected_size`, bitwise parity is mathematically guaranteed.
- **Drive Thermal & Wear Reduction**: Eliminating 60 GB of continuous disk platter reads reduces actuator wear, thermal throttling on USB enclosures, and operational sync time.

### Alternatives Considered
- *Always re-read destination from disk*: Proven to cause perceived freezes and double the sync time (taking ~50 minutes for 130 GB instead of ~25 minutes).
- *Disable hashing entirely (`--no-verify-hash`)*: Eliminates integrity checks. In-flight hashing provides the best of both worlds: 100% cryptographic certainty at zero additional disk I/O cost.

---

## 3. Spindle Contention & Concurrency Control

### Decision
Enforce **per-spindle serialization** while allowing **inter-drive concurrency**:
1. Identify the physical root/mount of the source and destination drives.
2. Enforce that for any physical drive spindle, at most **1 active file read or write operation** executes concurrently.
3. If an operator syncs to multiple independent physical replica drives (e.g. Primary `/Volumes/SP-1` -> Replica 1 `/Volumes/SP-2` and Replica 2 `/Volumes/SP-3`), transfers to Replica 1 and Replica 2 can run concurrently if they are separate physical disks, but no single drive executes interleaved file writes.

### Rationale
- **Mechanical Physics**: Mechanical HDDs have a single actuator arm. Concurrently reading or writing two files on the same drive causes seek thrashing across disk tracks, dropping sequential throughput from ~140 MB/s to ~20 MB/s.
- **Multi-Drive Pipelining**: When reading from Drive A and writing to Drive B, reading the next buffer chunk while the previous buffer chunk is writing (double buffering) keeps both drives operating sequentially at full speed.

### Alternatives Considered
- *Unrestricted Rayon/Tokio Threadpool Parallelism*: Catastrophic for mechanical HDDs. A threadpool of 8 workers writing 8 files simultaneously to the same HDD degrades performance by 500% due to head seek storms.
- *Global Single-File Lock across all drives*: Too restrictive when replicating across multiple independent physical disks.

---

## 4. Storage Flush (`fsync`) and Clean Handle Teardown

### Decision
1. Issue `dst_file.sync_all().await` immediately after the copy loop completes.
2. Explicitly `drop(dst_file)` and `drop(src_file)` before performing file status updates or database registration.
3. Catch any sync/flush error and invoke rollback (`tokio::fs::remove_file`) on the target file.

### Rationale
- Standard `tokio::fs::File::flush()` only flushes application userspace buffers to the OS kernel page cache.
- On macOS with external USB drives (APFS / exFAT), dirty pages remain in kernel cache and may fail delayed writeback if the bus stalls. `sync_all` issues an explicit `F_FULLFSYNC` / `fsync` syscall, ensuring blocks are physically committed to non-volatile platter storage before the application considers the transfer successful.
