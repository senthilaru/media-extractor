# Implementation Plan: Perceptual Fingerprinting & Hash Extraction Engine

**Branch**: `003-perceptual-deduplication` | **Date**: 2026-11-07 | **Spec**: [spec.md](spec.md)

**Input**: Revised feature specification from `specs/003-perceptual-deduplication/spec.md`

## Summary

The Perceptual Fingerprinting & Hash Extraction Engine computes cryptographic digests (BLAKE3 full payload hash and fast sample digest) and perceptual visual hashes (pure-Rust 2D DCT 64-bit pHash with 90°/180°/270° orientation invariance and low-entropy blank frame detection) on media assets and embeds the resulting `fingerprint` object directly into each file's normalized metadata JSON document (`MediaMetadataDocument`). Cross-file duplicate scanning, cluster formation, canonical master selection, and duplicate file marking/referencing are explicitly excluded per user requirements; downstream databases (e.g. MongoDB) leverage the emitted hashes for identical and near-duplicate query operations.

## Technical Context

**Language/Version**: Rust 1.82+ (2021 edition)

**Primary Dependencies**:
- `blake3` (1.8): SIMD-accelerated (ARM NEON / AVX-512) cryptographic hashing and sample digests with `rayon` and `mmap` support.
- `image` (0.25): Pure Rust image decoding with default features disabled, enabling `jpeg`, `png`, and `webp` decoders for bounded thumbnail generation.
- `nom-exif` (3.8) & `kamadak-exif` (0.6): Embedded RAW preview JPEG and thumbnail atom extraction without full sensor Bayer demosaicing.
- `rayon` (1.10): Work-stealing thread pool for parallel CPU-bound DCT math across M1 cores.
- `tokio` (1.41): Async runtime for bounded batch pipelining and channel streaming.
- `serde` (1.0) & `serde_json` (1.0): High-speed JSON serialization for MongoDB-ready document output.
- `thiserror` (2.0): Strongly-typed domain and infrastructure error definitions.
- `uuid` (1.11): Unique document identifier generation.
- `tracing` (0.1): Structured spans and extraction telemetry metrics.

**Testing**: `cargo test` (unit and integration tests with synthetic media fixtures, rotated variants, and corrupt headers), `criterion` (benchmarks).

**Target Platform**: macOS (Apple Silicon M1/M2/M3 unified memory, arm64) and Linux (x86_64, aarch64).

**Performance Goals**:
- $\ge 100$ files/sec per CPU core on perceptual visual fingerprinting (pHash) on compressed photo assets.
- $> 3.0\text{ GB/sec}$ throughput per core on cryptographic hashing with `blake3`.

**Constraints**:
- Memory consumption strictly bounded under 100 MB during batch streaming of 100,000+ items.
- Zero panics or unhandled termination on corrupt, truncated, or low-entropy media files.
- 100% safe Rust code with zero C/C++ FFI dependencies.

## Architecture Integration

1. **Domain Layer**:
   - Add `fingerprint: Option<MediaFingerprint>` to `MediaMetadataDocument` in `src/domain/metadata_models.rs`.
   - Update `MediaFingerprint` in `src/domain/dedup_models.rs` to provide JSON-friendly `phash_hex` accessor.
2. **Infrastructure Layer**:
   - `Blake3MediaHasher` in `src/infrastructure/blake3_hasher.rs` computes streaming BLAKE3 digest and fast sample digest.
   - `DctPerceptualHasher` in `src/infrastructure/phash.rs` computes 64-bit DCT pHash, rotated hashes, and variance.
   - `RasterDecoder` in `src/infrastructure/raster_decoder.rs` decodes directly to $32 \times 32$ luminance matrix.
3. **Application Layer**:
   - `MetadataExtractionEngine` in `src/application/metadata_engine.rs` invokes `hasher.fingerprint_file` when extracting metadata, embedding the `MediaFingerprint` directly into the resulting `MediaMetadataDocument`.
4. **CLI**:
   - Both `extract` and `extract-stream` subcommands automatically emit documents with the `fingerprint` section included.
   - No separate duplicate scanning, cluster formation, or duplicate file referencing occurs.
