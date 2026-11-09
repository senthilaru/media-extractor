# Phase 0 Research: Tiered Media Deduplication & Perceptual Fingerprinting Engine

**Feature Branch**: `003-perceptual-deduplication`  
**Date**: 2026-11-07  
**Status**: Completed  

---

## 1. Multi-Tier Deduplication Pipeline Architecture

### Decision
Implement a 4-tier progressive evaluation pipeline following the project Constitution:
- **Tier 1 (Attribute & Metadata Filtering)**: Partition media assets into candidate candidate sets using identical `file_size_bytes` (for exact duplicates) or temporal facets / aspect ratios (for visual near-duplicates).
- **Tier 2 (Fast Sample Digest)**: Compute a 64-byte sample digest combining file size, first 16 KB (header), and last 16 KB (trailer) using `blake3`. Candidates with different sample digests are immediately discarded.
- **Tier 3 (Full Cryptographic Digest)**: Stream full file bytes through `blake3::Hasher` in 64 KB chunks to guarantee 100% bitwise identity (`ExactBitwise` duplicate).
- **Tier 4 (Perceptual Visual Fingerprint)**: For non-exact candidates sharing similar aspect ratios or temporal windows, decode a downscaled $32 \times 32$ luminance thumbnail and compute a 64-bit DCT perceptual hash (`pHash`). Near-duplicates are identified when the Hamming distance $\le 10$ bits (similarity $\ge 84.4\%$).

### Rationale
- Computing cryptographic digests and perceptual hashes across millions of files is computationally expensive.
- In real-world media archives, over 95% of asset pairs differ in file size or early header bytes.
- Tiered evaluation ensures expensive raster decoding and DCT math are executed only on plausible candidates, reducing pairwise comparisons by $> 90\%$.

### Alternatives Considered
- *Single-Tier Perceptual Hashing (All-Pairs)*: Computing perceptual hashes for every file and comparing all pairs ($O(N^2)$) causes severe CPU thrashing and violates Constitution Principle I on multi-million asset archives.
- *Cryptographic-Only Deduplication*: Misses resized, re-encoded (JPEG to WebP), or rotated copies, failing User Story 2.

---

## 2. Cryptographic Hashing Engine for Exact Bitwise Identity

### Decision
Adopt `blake3` (v1.8) for both Tier 2 partial sample hashing and Tier 3 full cryptographic digests:
- Uses SIMD-accelerated Tree-Hashing (AVX-512 on x86, NEON on Apple Silicon M1/M2/M3).
- Delivers $> 3.5\text{ GB/sec}$ throughput per CPU core on NVMe storage, saturating hardware I/O bandwidth.
- Provides 256-bit cryptographic security with zero known collisions.
- Integrates zero-copy chunk streaming via `blake3::Hasher` and memory-mapped files (`memmap2` / bounded buffers) for large assets.

### Rationale
- `blake3` is 4x to 10x faster than SHA-256 and MD5 while providing cryptographic collision resistance.
- Naturally supports incremental updating and work-stealing parallel hashing via `rayon` for files exceeding 100 MB.

### Alternatives Considered
- *SHA-256 / SHA-1*: Standard in older tools, but throughput is limited to ~400 MB/s per core, creating an I/O bottleneck on Apple Silicon SSDs.
- *xxHash / MurmurHash*: High throughput, but non-cryptographic (susceptible to hash collisions on multi-terabyte datasets, risking data loss during automated deduplication).

---

## 3. Perceptual Visual Hashing (pHash) Algorithm

### Decision
Implement a pure-Rust Discrete Cosine Transform (DCT) 64-bit perceptual visual hasher with orthogonal rotation invariance and entropy guards:
1. **Luminance Normalization**: Convert decoded image pixels to an 8-bit grayscale $32 \times 32$ luminance matrix.
2. **Variance / Entropy Guard**: Compute the variance $\sigma^2$ across the 1024 grayscale pixels. If $\sigma^2 < 5.0$ (monochromatic, pure black, or blank frames), flag `is_low_entropy = true` and prevent visual clustering with other blank images unless bitwise identical.
3. **2D DCT-II Transform**: Compute the 2D Discrete Cosine Transform on the $32 \times 32$ matrix to separate spatial frequency components.
4. **Low-Frequency Extraction**: Extract the $8 \times 8$ low-frequency sub-matrix (64 coefficients), excluding the DC component at $(0,0)$ which represents global illumination.
5. **Median Binarization**: Compute the median of the 64 AC coefficients. Set bit $i = 1$ if coefficient $i > \text{median}$, else $0$, producing a 64-bit unsigned integer (`u64`).
6. **Rotation Invariance**: In addition to the canonical 0° hash, generate hashes for 90°, 180°, and 270° rotations by transposing and reversing the $32 \times 32$ matrix before DCT.
7. **Hamming Distance Comparison**: Measure bit differences using single-cycle hardware popcount:
   $$\text{distance} = (h_1 \oplus h_2).\text{count\_ones}()$$
   Two images are near-duplicates if $\min(\text{dist}_{0^\circ}, \text{dist}_{90^\circ}, \text{dist}_{180^\circ}, \text{dist}_{270^\circ}) \le 10$.

### Rationale
- DCT-based pHash is robust against JPEG recompression, contrast adjustments, modest color filtering, and up to 50% image scaling.
- Operating directly on $32 \times 32$ matrices requires $< 2\text{ KB}$ of memory per image and takes $< 0.1\text{ ms}$ per hash on Apple Silicon M1 cores.
- Pre-computing rotated hashes allows instantaneous detection of smartphone orientation flips without re-reading the source file.

### Alternatives Considered
- *Average Hash (aHash) / Difference Hash (dHash)*: Faster, but fragile against contrast shifts, gamma curves, and rotation.
- *Deep Learning / Embedding Vectors (ResNet / CLIP)*: Highly accurate for semantic similarity, but requires heavy ML dependencies (PyTorch/ONNX), multi-gigabyte models, high GPU/NPU memory, and violates Constitution Principle IV (no C/C++ FFI / pure Rust reliability).

---

## 4. Image Raster Decoding & Thumbnail Extraction

### Decision
Use `image` (v0.25) with default features disabled, enabling only pure-Rust format decoders (`jpeg`, `png`, `webp`):
- For compressed photos: decode directly to a bounded $32 \times 32$ grayscale thumbnail using `image::imageops::resize` with `FilterType::Nearest` or `Triangle` to maximize speed.
- For camera RAW formats (CR2, CR3, NEF, ARW, DNG, RAF, ORF): extract the embedded EXIF preview JPEG via `nom-exif` / `kamadak-exif` and decode the preview thumbnail rather than running slow Bayer raster demosaicing.
- For video files: inspect the container keyframe preview or rely on Tier 1-3 cryptographic and container atom matching.

### Rationale
- Camera RAW demosaicing can take 500ms to 2000ms per file and allocate 100+ MB of RAM for a single 45MP image. Extracting the embedded preview JPEG takes $< 5\text{ ms}$ and consumes $< 2\text{ MB}$.
- Keeps batch memory consumption strictly bounded under 100 MB.

### Alternatives Considered
- *Libraw / Libvips FFI*: Introduces unsafe C/C++ bindings, complex platform dependencies, and potential memory leaks, violating Constitution Principle IV.
- *Full-resolution raster decoding*: Consumes excessive memory (uncompressed RGBA rasters for 50MP files require 200 MB each) and slows throughput below the 100 files/sec target.

---

## 5. Multi-Index Hashing for Fast Near-Duplicate Search

### Decision
Implement Multi-Index Hashing (MIH) based on the Pigeonhole Principle:
- A 64-bit perceptual hash is split into $K = 4$ chunks of 16 bits: $[C_0, C_1, C_2, C_3]$.
- If two hashes have a Hamming distance $d \le 10$, by the Pigeonhole Principle:
  $$\lfloor d / K \rfloor = \lfloor 10 / 4 \rfloor \le 2$$
  At least one 16-bit chunk pair must have a Hamming distance $\le 2$.
- Maintain 4 hash lookup tables mapping 16-bit chunk keys to candidate media IDs.
- For each query hash, query the 4 tables (with 0, 1, or 2 bit flips), union candidate IDs, and compute exact 64-bit Hamming distances only on candidates.

### Rationale
- Reduces pairwise comparison complexity from $O(N^2)$ to $O(N \cdot M)$ where $M \ll N$.
- Scales easily to 100,000+ images in memory with $< 15\text{ MB}$ of RAM.

### Alternatives Considered
- *Brute-force pairwise search*: Feasible only for $N < 2,000$; for $N = 100,000$, requires 5 billion comparisons.
- *BK-Tree (Burkhard-Keller Tree)*: Good for discrete metric spaces, but has higher pointer overhead and poor cache locality compared to contiguous hash tables.

---

## 6. Deterministic Canonical Master Selection & Reclamation Scoring

### Decision
Implement a deterministic scoring function to select the single canonical master asset in each duplicate cluster:
$$\text{Score} = S_{\text{format}} + S_{\text{resolution}} + S_{\text{size}} + S_{\text{date}}$$
Where:
- $S_{\text{format}}$: `RawPhoto` (300 pts) > `Photo` (200 pts) > `Video` (100 pts).
- $S_{\text{resolution}}$: $\min(\text{width} \times \text{height} / 10000, 200)$ points.
- $S_{\text{size}}$: $\min(\text{file\_size\_bytes} / (1024 \times 1024), 100)$ points.
- $S_{\text{date}}$: Earlier capture timestamp gets higher priority (preserving original capture).
- Tie-breaker: Lexicographical sorting on canonical `file_path`.

Variant classification:
- `ExactBitwise`: Cryptographic hash matches canonical file.
- `ThumbnailVariant`: Perceptual match, but resolution is $\le 60\%$ of canonical file.
- `RecompressedVariant`: Perceptual match with similar resolution but smaller file size.
- `PerceptualVisualDuplicate`: Perceptual match with similar resolution and size.

Reclaimable bytes:
$$\text{ReclaimableBytes} = \sum_{v \in \text{Variants}} \text{file\_size\_bytes}(v)$$

---

## Summary of Crate Dependencies Added
- `blake3 = { version = "1.8", features = ["rayon", "mmap"] }`: Fast cryptographic and partial hashing.
- `image = { version = "0.25", default-features = false, features = ["jpeg", "png", "webp"] }`: Lightweight thumbnail decoding for perceptual hashing.
