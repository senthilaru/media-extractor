# Technical Research: Scan Progress Telemetry & High-Throughput Performance Optimization

**Feature**: `007-scan-progress-performance`  
**Date**: 2026-11-08  
**Status**: Completed  

---

## 1. Terminal Progress Visualization & ETA Tracking

### Context
When operators scan large media datasets (e.g. 1 TB with tens of thousands of media files), the tool previously output simple single-line `\r` telemetry heartbeats without overall progress percentage, estimated time of arrival (ETA), or data volume processed. When stdout is redirected or errors occur, line wrapping or output collision creates visual corruption.

### Decision
Adopt `indicatif` (v0.17) combined with standard TTY detection (`std::io::stderr().is_terminal()`) to provide:
1. **Interactive Mode (TTY)**: Multi-metric progress bar with spinner, active phase indicator, items counter (`{pos}/{len}`), data volume (`{bytes}/{total_bytes}`), throughput rates (`{per_sec}`, `{bytes_per_sec}`), elapsed time (`{elapsed}`), and dynamic ETA (`{eta}`).
2. **Non-Interactive Mode (Pipe / CI / Redirect)**: Automatically detect non-TTY stderr and emit discrete, periodic single-line telemetry records (every 5 seconds) without carriage returns or ANSI cursor control sequences.
3. **Structured Telemetry (JSON)**: Preserve existing `--format json` or `--json-telemetry` capability on stderr for automated supervisors.
4. **Quiet Mode (`-q` / `--quiet`)**: Fully suppress progress bars while preserving exit codes and fatal error reporting.

### Rationale
- `indicatif` is the standard Rust progress bar crate with battle-tested terminal width handling, zero-cost atomic counters, steady-tick spinners, moving-average throughput and ETA calculations, and clean multi-progress bar coordination.
- Isolating all progress bars to `stderr` guarantees that `stdout` output streams (JSON documents or piped text) remain 100% clean and valid for piping to tools like `jq` or file redirects.

### Alternatives Considered
- **Custom raw ANSI escapes (`\r\x1b[K`)**: Brittle across different terminal emulators, fails to handle terminal resizing, and requires custom implementations of moving-average ETA smoothing algorithms.
- **Log-only heartbeats (`tracing::info!`)**: Clutters terminal scrolling and makes it impossible to see an at-a-glance status bar.

---

## 2. Pipelined Concurrency & Dynamic Worker Dispatch

### Context
In the previous implementation, the extraction loop in `main.rs` sequentially awaited each batch of 500 items:
```rust
while let Some(batch) = stream.batches.recv().await {
    let docs = engine.extract_batch(batch, Some(&root)).await;
    ...
}
```
If a single batch contained large video files or slow RAW images, directory traversal stalled because the bounded batch channel filled up, leaving CPU cores idle and causing 13,000 files to take over 40 minutes (~5.4 files/sec).

### Decision
Refactor extraction orchestration into an asynchronous bounded pipeline:
1. **Decoupled Pipelining**: Directory discovery runs independently as an asynchronous producer task, streaming discovered items/batches into a bounded channel.
2. **Concurrent Batch Worker Pool**: Spawn a bounded pool of concurrent extraction workers (scaled to host CPU cores, e.g. `num_cpus` or configured `--workers`) using `tokio::sync::Semaphore` or worker tasks, so discovery and extraction overlap continuously.
3. **Bounded Memory Backpressure**: The queue between discovery and extraction is bounded (e.g. 50 batches or 10,000 items), preventing memory growth while ensuring workers never starve. Total resident memory remains strictly $< 500$ MB RAM.
4. **Rayon Work-Stealing Pool**: Heavy compute tasks (EXIF parsing, video container demuxing, raster decoding) continue to run on dedicated `rayon` worker threads via `spawn_blocking`, keeping Tokio runtime threads unblocked per Constitution Principle I.

### Rationale
Pipelining discovery, extraction, and database persistence eliminates serial synchronization stalls. While extraction workers are parsing metadata for batch $N$, the directory walker is traversing batch $N+1$, and the database sink is asynchronously flushing batch $N-1$.

### Alternatives Considered
- **Unbounded `tokio::spawn` per file**: Exhausts file descriptors, causes massive context-switching overhead, and risks out-of-memory panics on 500K+ file trees.
- **Synchronous batch-by-batch execution**: Simpler to write, but creates severe pipeline bubbles and starves hardware I/O and compute cores.

---

## 3. High-Speed Scanning & Large Media Hashing Strategy

### Context
Blake3 cryptographic hashing previously read entire file payloads in 64 KB chunks (`BufReader`), and perceptual visual hashing decoded 32x32 thumbnails for all images. On a 1 TB dataset, reading 1 TB of data from disk at 50–100 MB/s takes hours just in raw I/O transfer time.

### Decision
1. **Configurable Fast Mode (`--fast`)**:
   - Extract format container metadata, EXIF tags, dimensions, durations, and timestamps without reading the body of the media file for bitwise cryptographic or perceptual hashing.
   - Sets extraction throughput target to $> 50$ files/sec on SSD storage, enabling a 10x–50x speed improvement for inventory and metadata indexing.
2. **File Size Threshold for Hashing (`--max-hash-size <MB>`)**:
   - Introduce a configurable threshold (default: 100 MB). Media files larger than this threshold have full-file bitwise hashing skipped (or use fast 32 KB sample digests) unless `--full-hash` is explicitly requested.
   - Large video files (e.g. 5 GB 4K video) extract QuickTime/MP4 duration and codec headers in $< 5$ milliseconds instead of spending minutes reading 5 GB from disk.
3. **Memory-Mapped Blake3 Hashing (`update_mmap_rayon`)**:
   - For files that are hashed and exceed 64 KB, use `blake3::Hasher::update_mmap_rayon(path)` to leverage OS page cache and multi-core SIMD tree hashing, dramatically improving hashing throughput.

### Rationale
Most media organization and metadata extraction use cases only need timestamps, camera settings, GPS coordinates, dimensions, and video durations. Decoupling metadata extraction from deep cryptographic and perceptual hashing allows operators to choose between instant high-speed cataloging and deep deduplication verification.

### Alternatives Considered
- **Skipping large video files entirely**: Unacceptable because video duration, codec, and creation timestamp are essential metadata.
- **Always hashing all bytes**: Unacceptable performance on multi-gigabyte files (violates user requirement to improve speed on 1 TB datasets).

---

## 4. Unified Progress State & Post-Scan Execution Summary

### Context
Constitution Principle III mandates comprehensive measurability and observability for all I/O and external service operations, including live progress metrics and final elapsed time summaries (`timetaken`, processed tallies, throughput rates, and error counts).

### Decision
1. **Atomic Progress Aggregator (`ScanProgressState`)**:
   - Thread-safe state tracking `discovered_items`, `discovered_bytes`, `processed_items`, `processed_bytes`, `db_persisted_items`, `errors_count`, and `start_time`.
   - Computes moving-average throughput rates (`items_per_sec`, `mb_per_sec`) and dynamic ETA:
     $$\text{ETA} = \frac{\text{Remaining Items}}{\text{Moving Average Items/Sec}}$$
2. **Execution Summary Report**:
   - Emits a clean, structured summary on stderr at completion or on Ctrl+C termination:
     ```text
     ============================================================
     Scan & Extraction Summary
     ------------------------------------------------------------
     Target Path:        /spsa/temp
     Performance Mode:   Fast (Metadata-only, skip deep hashing)
     Total Elapsed Time: 3m 42s
     Total Files:        13,420 processed (13,420 discovered)
     Total Data Volume:  1.02 TB
     Average Throughput: 60.4 files/sec (4.8 GB/min)
     Database Ingested:  13,420 upserted (0 failed)
     Errors / Warnings:  0 errors, 4 warnings
     ============================================================
     ```

### Rationale
Provides operators with complete confidence and clarity from the moment the scan starts until it completes, ensuring zero unmonitored silent execution.

### Alternatives Considered
- **Standard tracing log only**: Lacks visual grouping and is easily overlooked among debug messages.
