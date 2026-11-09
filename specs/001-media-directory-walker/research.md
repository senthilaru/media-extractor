# Research & Technology Decisions: Media Directory Walker

**Feature**: `001-media-directory-walker`  
**Date**: 2026-11-07  
**Status**: Completed  

---

## 1. Directory Traversal Engine

### Decision
Use `jwalk` (built on `rayon` work-stealing parallelism) encapsulated within an asynchronous bridge worker that streams discovered items into a bounded `tokio::sync::mpsc` channel.

### Rationale
- **Multi-Core Throughput**: On modern NVMe SSDs and Apple Silicon M1 unified memory architecture, single-threaded traversal (`std::fs::read_dir` or `walkdir`) is bottlenecked by sequential syscall latency. `jwalk` dynamically parallelizes directory traversal across threads using Rayon work-stealing, achieving 15,000 to 50,000+ files/sec on local SSDs.
- **Cycle Detection**: `jwalk` natively supports cycle detection for symbolic links, preventing infinite recursion out of the box.
- **Selective Traversal & Filtering**: Custom filter closures allow early pruning of hidden directories (e.g., `.git`, `.thumbnails`, `.Trash`) before reading metadata for all child files, dramatically reducing unnecessary syscalls.

### Alternatives Considered
- `walkdir`: Simple and robust, but single-threaded. On deep archives with millions of files, single-threaded traversal takes 4x–8x longer and fails the throughput target (SC-004).
- `tokio::fs::read_dir`: Pure async filesystem operations. However, OS filesystem calls on Unix are fundamentally blocking at the kernel level; `tokio::fs` delegates each call to `spawn_blocking`, incurring thread scheduling overhead that degrades performance on millions of tiny directory reads.
- `ignore`: Fast and parallel (used by `ripgrep`), but tailored for gitignore parsing with significant parsing overhead unneeded for media library crawling.

---

## 2. Backpressured Batch Streaming Architecture

### Decision
Implement flow control using a bounded `tokio::sync::mpsc::channel(capacity)` paired with an asynchronous batching aggregator (`tokio_stream::StreamExt::chunks_timeout` or custom bounded batch accumulator).

### Rationale
- **Strict Memory Bounding**: A scan of 5,000,000 media files would require ~1 GB of RAM if fully materialized in memory. With a bounded channel (e.g., channel buffer of 1,000 batches of 500 items = 500,000 items in flight max, ~50 MB RAM), traversal automatically pauses (`await` or backpressure signal) when downstream consumers (metadata extractors, MongoDB writers) are busy, guaranteeing memory stays strictly under the 100 MB constitutional ceiling (SC-003).
- **Zero Startup Latency**: Batches begin emitting as soon as the first `batch_size` (e.g., 250–500) items are accumulated, delivering the first batch to consumers within tens of milliseconds (satisfying SC-002: < 2 seconds).

### Alternatives Considered
- Unbounded channel (`tokio::sync::mpsc::unbounded_channel`): Risks uncontrolled heap growth and out-of-memory panics if consumers process slower than the walker produces.
- Ring buffer / Shared-memory queue (`crossbeam-channel` alone): Excellent for synchronous inter-thread communication, but lacks native async `await` integration for non-blocking coordination across Tokio tasks.

---

## 3. High-Performance Media Extension Classification

### Decision
Utilize `phf` (compile-time Perfect Hash Function) to build a static set of supported photo and video extensions, evaluated against case-folded extension slices without heap allocation.

### Rationale
- **Zero Heap Allocations on Hot Path**: Every file inspected during a multi-million file crawl undergoes extension matching. By borrowing the file extension from `Path::extension()`, matching against an ASCII lowercase buffer or case-insensitive ASCII comparison, and querying a compile-time static perfect hash table, lookup time is $O(1)$ with 0 bytes allocated per file.
- **Categorization**: The hash set maps extensions directly to a `MediaCategory` enum (`Photo`, `Video`, `RawPhoto`).

### Alternatives Considered
- `regex` matching: Powerful, but orders of magnitude slower; introduces unnecessary overhead and regex state evaluation per path.
- `HashSet<String>` at runtime: Requires heap allocations and runtime hashing overhead on every lookup.
- MIME type sniffing (`infer` / magic bytes): Reading file headers on all millions of files during discovery defeats high-throughput traversal; header inspection is deferred to downstream metadata extraction workers.

---

## 4. Hardware Optimization for Apple Silicon (M1)

### Decision
Tune Rayon and Tokio runtime threads to align with Apple Silicon M1 topology (8 physical cores: 4 Firestorm Performance cores + 4 Icestorm Efficiency cores).

### Rationale
- **Core Partitioning**: 
  - Directory scanning and I/O coordination are assigned to a bounded thread pool sized to `available_parallelism()` (defaulting to 8 threads on standard M1).
  - Rayon's work-stealing algorithm naturally distributes work, preventing idle starvation while avoiding excessive thread context switching.
- **OS Page Cache Utilization**: M1 unified memory architecture provides high memory bandwidth (~68 GB/s). Keeping traversal lightweight ensures file metadata stays warm in the macOS unified buffer cache for subsequent extractor processes.

### Alternatives Considered
- Spawning unbounded OS threads per directory: Causes severe thread thrashing, OS file descriptor exhaustion (`EMFILE`), and high latency.
- Pinning threads to performance cores exclusively (`thread_affinity`): Complex and brittle across varied Apple Silicon models (M1, M1 Pro, M1 Max, M2, M3); standard OS scheduling with work-stealing performs optimally without kernel-level thread pinning.

---

## 5. Symlink Cycle Detection & Fault Isolation

### Decision
Track visited filesystem device and inode identifiers (`(u64, u64)` via `std::os::unix::fs::MetadataExt`) in a compact concurrent set for directory symlinks. Map all filesystem errors into strongly-typed `ScanErrorRecord` items without aborting traversal.

### Rationale
- **Deterministic Loop Prevention**: Symlinks pointing to parent or ancestor directories cause infinite recursion. Comparing `(dev, ino)` guarantees cycles are identified regardless of path aliases or relative navigation.
- **Constitutional Fault Boundary**: Adhering to Principle V, errors such as `std::io::ErrorKind::PermissionDenied` or `NotFound` (e.g. vanished files) are captured as diagnostic records and emitted to the telemetry/error stream, allowing the scanner to process all remaining valid paths.

### Alternatives Considered
- Path canonicalization for cycle checks: Resolving `std::fs::canonicalize` on every folder incurs substantial disk syscall overhead. Comparing raw `(dev, ino)` pairs from `DirEntry::metadata()` is orders of magnitude faster.
- Aborting on first I/O error: Fails enterprise requirements and violates Constitution Principle V.

---

## 6. Observability & Telemetry

### Decision
Instrument hot paths with `tracing` spans and counters, emitting periodically aggregated metrics (directories scanned, files discovered, items/sec, queue depth) via an atomic heartbeat reporter.

### Rationale
- Emitting a log message per file would saturate I/O and bottleneck performance. Using atomic counters (`AtomicU64`) on the hot path and an asynchronous heartbeat timer (e.g., every 1000ms) to emit structured telemetry provides real-time visibility with near-zero performance cost.
