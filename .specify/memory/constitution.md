<!--
SYNC IMPACT REPORT
==================
Version Change: 1.1.0 -> 1.2.0
Ratification Date: 2026-11-07
Last Amended Date: 2026-11-08

Modified Principles:
- III. Comprehensive Measurability & Observability (mandated live progress indicators and elapsed time taken metrics on all I/O and external service operations)

Added Principles:
- None

Added Sections / Standards:
- Performance & Operational Standards: I/O & External Service Progress & Timing Telemetry

Removed Sections:
- None

Follow-up TODOs:
- None (all placeholders resolved)
-->

# Metadata Extractor Constitution

## Core Principles

### I. High-Throughput I/O & Hardware-Conscious Concurrency
- The system MUST prioritize non-blocking, asynchronous I/O (`tokio`) for file system traversal and external operations, combined with dedicated work-stealing thread pools (`rayon`) for compute-intensive tasks.
- Concurrency models MUST be tuned for Apple Silicon M1 architecture (balancing high-performance and high-efficiency cores) without CPU oversubscription or thread thrashing.
- Blocking file operations or CPU-heavy decoding/hashing MUST NEVER execute on asynchronous runtime worker threads; compute workloads MUST be dispatched to dedicated blocking/compute pools.
- Memory consumption MUST be strictly bounded using streaming pipelines and backpressured channels to avoid memory exhaustion when processing millions of media files or multi-gigabyte RAW assets.
- *Rationale*: Processing millions of photo and video files across diverse media formats requires saturating available hardware I/O and CPU bandwidth without causing latency spikes, executor starvation, or out-of-memory panics.

### II. Domain-Driven Design & SOLID Boundaries
- The architecture MUST strictly adhere to Domain-Driven Design (DDD) and Hexagonal/Clean Architecture principles, separating Domain Models from Application Workflows and Infrastructure Adapters.
- Domain logic (e.g., media entities, deduplication fingerprints, metadata domain types) MUST remain pure and independent of external libraries, file systems, or database implementations.
- Every major infrastructure component (file system scanners, EXIF parsers, perceptual hashers, video metadata extractors, MongoDB storage) MUST be abstracted behind explicit Rust traits (ports) adhering to the Single Responsibility and Interface Segregation principles.
- Concrete third-party dependencies MUST NOT leak into domain interfaces; external data MUST be transformed into domain value objects via dedicated mappers.
- *Rationale*: Formats, decoders, and database drivers evolve over time. Strict decoupling ensures individual extractors, storage backends, or scanning algorithms can be tested, refactored, or replaced without destabilizing domain logic.

### III. Comprehensive Measurability & Observability
- Every major external interaction—including file discovery, file descriptor acquisition, metadata parsing, hashing, and database roundtrips—MUST be measured and observable.
- All operations involving file system I/O, network transfers, or external service calls (e.g., database connections, index builds, batch writes, remote queries) MUST visibly display live progress metrics and elapsed time taken (e.g., elapsed duration, processing rates in items/sec or MB/sec, and item counters) via standard error telemetry or structured progress heartbeats.
- System components MUST emit structured telemetry (timings, throughput rates, error tallies, payload sizes) using structured tracing spans and metrics compatible with open standards.
- Metric instrumentation MUST incur minimal overhead on the hot path (zero-allocation counters/histograms where possible).
- Structured logs and tracing events MUST include contextual file identifiers and batch correlation IDs while strictly avoiding the leakage of sensitive user data or unmasked credentials.
- *Rationale*: Operating on multi-million asset workloads requires granular visibility into execution bottlenecks, storage latency profiles, and decoder failure distributions to facilitate data-driven optimization and debugging, while live progress and timing ensure operators maintain continuous visibility during heavy I/O operations.

### IV. Idiomatic Rust & Zero-Cost Reliability
- All code MUST adhere to idiomatic Rust standards, verified by strict compiler warnings, `rustfmt` formatting, and `clippy` lints enforced in CI.
- Memory safety and thread safety MUST be guaranteed via Rust's type system; `unsafe` code is strictly FORBIDDEN unless accompanied by explicit formal justification, zero-copy performance benchmarks, and isolation within audited boundaries.
- Errors MUST be represented as strongly-typed domain or infrastructure error enums (utilizing `thiserror` for library boundaries and `anyhow` for top-level application orchestration).
- Production code MUST NEVER call `unwrap()`, `expect()`, or trigger unhandled panics on untrusted or external inputs; every error path MUST be handled deterministically.
- *Rationale*: The safety and performance guarantees of Rust are central to building a bulletproof pipeline capable of handling millions of untrusted files without crashing or leaking resources.

### V. Fault Isolation & Resilient Batch Ingestion
- Processing individual files MUST be entirely fault-isolated; corrupt headers, unsupported encodings, incomplete transfers, or decoder panics/timeouts MUST NEVER crash the broader batch process.
- The pipeline MUST route unreadable, corrupted, or unsupported media files to a structured quarantine log or error repository with explicit diagnostic reasons, allowing batch processing to proceed uninterrupted.
- Database writes and external network operations MUST implement robust retry mechanisms with exponential backoff and circuit-breaking semantics.
- *Rationale*: Across millions of heterogeneous photo and video files from Android, iPhone, DSLRs, and legacy archives, data corruption and non-standard metadata are expected operational norms rather than exceptional termination states.

### VI. Zero Hardcoded Credentials & Mandatory Externalization
- Plaintext secrets, passwords, tokens, API keys, or database credentials MUST NEVER be hardcoded in application source code, automated test suites, configuration templates, or Markdown documentation files.
- All credentials MUST be externalized and resolved strictly through environment variables, external configuration profiles, or orchestration secret providers per 12-Factor principles.
- Operational environments MUST support distinct configuration profiles (such as `local` and `production`), where production strictly enforces authenticated endpoints and fails fast upon missing or default secrets.
- Logging, error formatting, and diagnostic telemetry MUST automatically mask all credentials (e.g., via `sanitized_uri()` or `***` redaction); unredacted secrets MUST NEVER be written to stdout, stderr, or persistent log files.
- Commit history and repository assets MUST be audited to guarantee that zero credentials leak into version control.
- *Rationale*: Hardcoding credentials introduces grave security, compliance, and operational risks. Strict externalization ensures deployments remain secure across diverse environments without accidental exposure in source repositories or log aggregation pipelines.

## Performance & Operational Standards

- **Hardware Concurrency Optimization**: Thread pool configurations MUST adapt dynamically to host hardware profiles (e.g., Apple Silicon M1 core counts and memory tiers), maintaining optimal core utilization while leaving sufficient headroom for system I/O.
- **Deduplication Pipeline Hierarchy**: Duplicate detection MUST follow a tiered evaluation strategy—evaluating fast metadata and file attributes before computing expensive perceptual hashes (`phash`) and cryptographic digests.
- **Resource Streaming & Bounded Buffers**: Media payloads MUST be ingested via stream-based readers or bounded memory-mapped buffers. Full-file memory buffering of large video or RAW image containers is prohibited.
- **Database Persistence Efficiency**: Metadata records MUST be written to MongoDB using bulk write operations, connection pooling, and backpressured ingestion channels to balance write throughput against database ingestion limits.
- **Credential Externalization & Secret Sanitization**: All database connections and third-party integrations MUST source authentication secrets from ambient environment variables or secured configuration profiles. In-memory connection URIs and error traces MUST mask credentials before emitting any diagnostic output. Automated test harnesses MUST use mock credentials, placeholders, or environment overrides rather than embedding secrets into test source code.
- **I/O & External Service Progress & Timing Telemetry**: Every long-running I/O traversal, batch extraction, or external service call (e.g. database connections, index verification, bulk writes, network queries) MUST track and surface real-time execution metrics. This includes periodic heartbeat progress indicators during active operations and final elapsed time summaries (`timetaken`, processed tallies, throughput rates, and error counts) upon completion, ensuring zero unmonitored silent execution.

## Quality Gates & Engineering Workflow

- **Automated Validation Gates**: Every code contribution MUST pass automated checks prior to merge, including `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, secret leakage scanning, and comprehensive test suites.
- **Testing Requirements**:
  - Unit tests MUST cover domain rules, value object validations, and parser error paths.
  - Contract and integration tests MUST validate infrastructure adapters (such as MongoDB storage drivers and file scanner behaviors) using test harnesses or mocks.
  - Regression fixtures MUST include corrupt, truncated, and edge-case media files to continuously test fault boundaries.
- **Benchmarking Hot Paths**: Performance-critical paths (scanner directory traversal, hashing routines, metadata decoding) MUST be benchmarked with statistical tooling (e.g., `criterion`) to prevent performance regressions.

## Governance

This Constitution represents the authoritative architectural and operational standard for the Media Metadata Extractor project. All feature specifications, design plans, and code implementations MUST comply with the rules established herein.

- **Supremacy & Precedence**: In any architectural disagreement or design review, this Constitution takes precedence over ad-hoc conventions or conflicting implementation plans.
- **Amendment Procedure**: Amendments to this Constitution require a documented RFC detailing the architectural rationale, compatibility implications, and migration plan, followed by formal consensus and an updated constitution version.
- **Versioning Policy**: This Constitution adheres to semantic versioning:
  - **MAJOR**: Removal, fundamental reversal, or breaking alteration of core principles or governance policies.
  - **MINOR**: Addition of new principles, sections, or materially expanded operational standards.
  - **PATCH**: Non-semantic clarifications, typographical corrections, or wording refinements.
- **Compliance Enforcement**: All Pull Requests and architectural reviews MUST verify compliance with these principles. Justification is required for any complexity or design tradeoff that borders on constitutional constraints.

**Version**: 1.2.0 | **Ratified**: 2026-11-07 | **Last Amended**: 2026-11-08
