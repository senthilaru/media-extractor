# Implementation Plan: Visual Semantic Recognition, Face Search & Smart Albums

**Branch**: `025-visual-semantic-recognition` | **Date**: 2026-11-22 | **Spec**: [specs/025-visual-semantic-recognition/spec.md](spec.md)

**Input**: Feature specification from `specs/025-visual-semantic-recognition/spec.md`

## Summary

This feature extends the metadata extraction engine with on-device computer vision capabilities: multi-label physical object detection (e.g. teddy bears, bicycles, phones, flowers), event/occasion scene classification (e.g. birthday parties, Halloween, Christmas), facial recognition with emotion detection (smile, happy, crying, neutral), in-image optical text extraction (OCR), and virtual smart album synthesis. The implementation follows a strict 4-stage incremental rollout, runs 100% locally on-device without cloud leakage, and maintains virtual albums without duplicating physical files on disk.

## Technical Context

**Language/Version**: Rust 1.75+ (Edition 2021)

**Primary Dependencies**: `tokio` 1.41, `rayon` 1.10, `image` 0.25, `mongodb` 3.2, `serde` 1.0, `thiserror` 2.0, `tracing` 0.1, `clap` 4.5, `indicatif` 0.17

**Storage**: MongoDB (extending `media_assets` with `visual` facets; dedicated `person_clusters` and `smart_albums` collections) + local single-copy filesystem master paths

**Testing**: `cargo test` (unit, integration, and benchmark tests)

**Target Platform**: macOS (Apple Silicon M1 optimized / POSIX)

**Project Type**: CLI application & asynchronous batch extraction daemon

**Performance Goals**: Stream-based batch inference, bounded memory queues, sub-500ms multi-modal query execution across 100,000+ assets

**Constraints**: Strict non-destructive media immutability (original files remain 100% read-only and bit-for-bit untouched), strict on-device local execution (zero cloud upload), non-blocking asynchronous pipeline, constitution-compliant DDD architecture, zero disk duplication for albums

**Scale/Scope**: Multi-terabyte libraries with 100,000+ photos and videos across heterogeneous cameras (DSLR, iPhone, Android)

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

| Principle | Requirement | Design Status | Compliance Notes |
| :--- | :--- | :--- | :--- |
| **I. High-Throughput I/O & Concurrency** | Dedicated compute pools for CPU/GPU workloads; bounded memory streams | **PASS** | Visual inference is executed via `rayon` worker pools and bounded channels; async `tokio` runtime is never blocked by image decoding. |
| **II. DDD & SOLID Boundaries** | Pure domain entities; infrastructure decoupled via ports/traits | **PASS** | Domain types (`VisualSemanticMetadata`, `PersonCluster`, `SmartAlbum`) are pure. Abstracted behind `VisualInferencePort` traits. |
| **III. Measurability & Observability** | Real-time progress bars, throughput rates, elapsed time summaries | **PASS** | Visual enrichment displays live items/sec and elapsed metrics via `indicatif`; structured spans track model latencies. |
| **IV. Idiomatic Rust & Reliability** | Strongly-typed errors, zero unhandled panics on corrupt inputs | **PASS** | Errors use `thiserror`; corrupt/unsupported images are safely logged and skipped without terminating batch jobs. |
| **V. Fault Isolation & Archive Protection** | Corrupted images must not crash pipelines; strictly non-destructive | **PASS** | Image decoding and inference are strictly read-only; zero bytes are ever written into original canonical master files. |
| **VI. Zero Hardcoded Credentials** | Masked credentials; externalized configuration | **PASS** | All MongoDB credentials resolve through ambient environment variables and sanitized URIs. |

## Project Structure

### Documentation (this feature)

```text
specs/025-visual-semantic-recognition/
├── spec.md              # Feature specification & user stories
├── plan.md              # This file (implementation plan)
├── research.md          # Technical decisions and architecture research
├── data-model.md        # Domain models, value objects, and MongoDB indexes
├── quickstart.md        # Verification guide and test scenarios
├── contracts/
│   └── cli-contract.md  # CLI subcommand contracts (enrich, faces, search, albums)
├── checklists/
│   └── requirements.md  # Specification quality checklist
└── tasks.md             # Implementation tasks (/speckit-tasks output)
```

### Source Code Architecture

```text
src/
├── domain/
│   ├── visual_models.rs       # VisualSemanticMetadata, DetectedObject, DetectedFace, ExtractedTextSnippet
│   ├── person_cluster.rs      # PersonCluster entity, cosine similarity, clustering logic
│   ├── smart_album.rs         # SmartAlbum entity and search criteria
│   └── ports.rs               # VisualInferencePort, MetadataStorageSink extensions
├── application/
│   ├── visual_service.rs      # Orchestrates visual enrichment batches over catalog
│   ├── face_service.rs        # Face clustering and person identity management
│   ├── search_service.rs      # Multi-modal search aggregator (people, objects, text, dates)
│   └── album_service.rs       # Smart album generation and synthesis
├── infrastructure/
│   ├── vision_adapter.rs      # Local computer vision inference adapter
│   └── mongodb_sink.rs        # MongoDB persistence for visual facets, person clusters, albums
└── main.rs                    # CLI commands: enrich, faces, search, albums

tests/
├── test_visual_enrichment.rs  # End-to-end visual object and occasion extraction tests
├── test_face_clustering.rs    # Face embedding clustering and naming tests
├── test_ocr_text_search.rs    # In-image word extraction and search tests
└── test_smart_albums.rs       # Virtual album synthesis and single-copy invariant tests
```

**Structure Decision**: Monolithic single-project CLI with clean DDD boundaries separating domain models, application orchestration services, and infrastructure adapters.

## Complexity Tracking

> *Zero constitution violations. No complexity exemptions required.*

| Aspect | Selected Approach | Simpler Alternative Rejected Because |
| :--- | :--- | :--- |
| **Virtual Albums** | Database-backed query manifests | Physical symlinks/hardlinks fail on exFAT/external drives and violate the single-copy canonical master invariant. |
| **Local Inference** | On-device lightweight models | Cloud APIs violate user privacy and require internet connectivity for terabyte archives. |
| **Phased Pipeline** | 4-stage decoupled extractors | Monolithic models are too slow for high-volume batch processing on consumer hardware. |
