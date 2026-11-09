# Implementation Plan: Scan Progress Telemetry & High-Throughput Performance Optimization

**Branch**: `007-scan-progress-performance` | **Date**: 2026-11-08 | **Spec**: [spec.md](spec.md)

**Input**: Feature specification from `specs/007-scan-progress-performance/spec.md`

## Summary

This feature resolves the operator visibility and performance bottlenecks experienced when scanning large-scale (1 TB+) media archives. It introduces:
1. **Interactive Real-Time Progress & ETA**: Live terminal progress display using `indicatif` on standard error showing items discovered, items processed, data volume (MB/GB), throughput rates (items/sec and MB/sec), elapsed duration, and dynamic moving-average ETA, with clean fallback for non-interactive streams.
2. **High-Throughput Pipelined Concurrency**: Decouples directory discovery from extraction and persistence through an asynchronous bounded worker pipeline with Rayon compute pools, eliminating sequential batch blocking.
3. **High-Speed Fast Mode & Size-Tiered Hashing**: Adds `--fast` mode (metadata-only without reading full payloads for bitwise/perceptual hashing) and `--max-hash-size` thresholding, achieving $> 50$ files/sec (>10x throughput speedup) on large media datasets.
4. **Execution Summary Report**: A structured boxed audit report printed on standard error at completion or upon Ctrl+C cancellation.

---

## Technical Context

**Language/Version**: Rust 2021 (Rust 1.80+)  
**Primary Dependencies**:
- `indicatif = "0.17"` (terminal progress bars, multi-progress, spinners, ETA calculation)
- `tokio = { version = "1.41", features = ["full"] }` (asynchronous runtime, bounded channels, signals)
- `rayon = "1.10"` (work-stealing compute thread pool for parallel extraction)
- `blake3 = { version = "1.8", features = ["rayon", "mmap"] }` (memory-mapped cryptographic hashing)
- `clap = { version = "4.5", features = ["derive", "env"] }` (CLI flag parsing)
- `tracing = "0.1"` (structured logging)

**Storage**: Local filesystem traversal, MongoDB 4.4+ (optional batch persistence sink)  
**Testing**: `cargo test` (unit and integration tests), `cargo bench` (throughput benchmarks)  
**Target Platform**: macOS (Apple Silicon M1 hardware-conscious concurrency), Linux x86_64  
**Project Type**: High-performance CLI tool & asynchronous batch ingestion pipeline  
**Performance Goals**:
- Fast mode throughput $> 50$ items/sec on SSD storage (10x-50x speedup over deep bitwise hashing).
- Live progress update interval at ~4 fps (250ms) with $< 1\%$ CPU telemetry overhead.
- Memory consumption strictly bounded $< 500$ MB RAM across 1 TB+ media datasets.  
**Constraints**:
- Strictly 100% safe Rust; zero unhandled panics or unwraps.
- Complete separation of progress telemetry (`stderr`) from output data streams (`stdout`).
- Zero unmonitored silent execution per Constitution Principle III.

---

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Check | Status |
|---|---|---|
| **I. High-Throughput I/O & Hardware-Conscious Concurrency** | Traversal runs asynchronously on Tokio; compute extraction runs on Rayon work-stealing threads; bounded channels prevent memory blowup; fast mode bypasses redundant disk reads. | **PASS** |
| **II. Domain-Driven Design & SOLID Boundaries** | `ScanProgressSnapshot`, `ExecutionSummary`, and `ExtractionMode` reside in domain models; `indicatif` terminal adapter lives in infrastructure behind a clean progress trait. | **PASS** |
| **III. Comprehensive Measurability & Observability** | Progress bars provide live items, bytes, throughput (items/sec and MB/sec), elapsed time, and ETA; final execution summary printed on completion or cancellation. | **PASS** |
| **IV. Idiomatic Rust & Zero-Cost Reliability** | Thread-safe atomic counters (`AtomicU64`); strongly typed domain errors; graceful signal handling (Ctrl+C). | **PASS** |
| **V. Fault Isolation & Resilient Ingestion** | Extraction errors on individual corrupt files increment error counters without halting the pipeline or crashing progress bars. | **PASS** |
| **VI. Zero Hardcoded Credentials & Secret Sanitization** | Connection strings and profile secrets continue to be masked via `sanitized_uri()` in all summary and telemetry outputs. | **PASS** |

---

## Project Structure

### Documentation (this feature)

```text
specs/007-scan-progress-performance/
├── plan.md              # Implementation plan (this file)
├── research.md          # Technical research & architectural decisions
├── data-model.md        # Entities: ScanProgressSnapshot, ExecutionSummary, ExtractionMode
├── quickstart.md        # Executable validation guide & test scenarios
└── contracts/
    └── cli-progress-performance-contract.md  # CLI flags and telemetry contracts
```

### Source Code Architecture

```text
Cargo.toml                                 # Add indicatif = "0.17"
src/
├── domain/
│   ├── mod.rs
│   ├── progress_models.rs                 # ScanProgressSnapshot, ExecutionSummary, ExtractionMode, OperationalPhase
│   └── ports.rs                           # ProgressReporter trait definition
├── application/
│   ├── mod.rs
│   ├── progress_tracker.rs                # Atomic progress aggregator & ETA moving-average calculator
│   └── pipeline.rs                        # Concurrent pipelined extraction orchestrator (decoupled discovery & workers)
├── infrastructure/
│   ├── mod.rs
│   ├── terminal_progress.rs               # Indicatif TTY adapter & non-TTY discrete log fallback
│   └── blake3_hasher.rs                   # Memory-mapped hashing & size-threshold optimizations
└── main.rs                                # CLI flags (--fast, --max-hash-size, --workers), signal handling, wiring
```

---

## Complexity Tracking

No constitutional violations or design exceptions required. All designs adhere to domain-driven hexagonal architecture and existing project standards.
