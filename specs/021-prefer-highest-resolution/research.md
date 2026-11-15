# Research: Resolution-Aware Deduplication Quality Selection

## 1. Quality Ranking Hierarchy

### Decision
Implement a deterministic `MediaQualityRank` comparison function used during candidate evaluation in deduplication clustering:
1. **Total Pixel Count ($W \times H$)**: Primary comparison. Higher resolution strictly wins over lower resolution.
2. **File Size (Bytes)**: Secondary comparison when pixel counts are equal or missing. Larger file size (higher bitrate / lower compression artifacts) wins.
3. **Format Precedence**: Tertiary comparison (RAW > Lossless PNG/TIFF > Lossy JPEG/HEIC/WebP > Video > Thumbnail/Sidecar).
4. **Metadata Completeness**: Quaternary comparison (presence of EXIF capture timestamp, GPS geolocation tags).
5. **Storage / Drive Priority**: Final tie-breaker when quality is identical (`Primary Master` > `Replica` > `Staging` > `Unknown`, then deterministic Doc ID / Path).

### Rationale
- Users explicitly need small/low-resolution images (thumbnails, compressed social media shares, QuickLook previews) pruned in favor of full-resolution originals.
- When pixel dimensions match (e.g. 1920x1080), file size is the most reliable indicator of image quality (JPEG compression ratio, chroma subsampling, bitrate).
- Format precedence ensures digital camera RAW negatives are never discarded in favor of smaller JPEG previews.

### Alternatives Considered
- *Perceptual Quality Metametrics (e.g., SSIM, BRISQUE, NIQE)*: Rejected due to severe CPU overhead across millions of assets; violates Apple Silicon M1 high-throughput efficiency principles in the Constitution.
- *Strict File Size First*: Rejected because an uncompressed or bloated low-res BMP/TIFF can be larger in byte size than an efficiently encoded high-res HEIC/JPEG. Pixel count ($W \times H$) must come first.

---

## 2. Integration with `evaluate_content_group` & Universal Classification

### Decision
Update `dedup_engine.rs` (`evaluate_content_group` and `score_canonical_candidate`):
- Extract `dimensions` (`width`, `height`) from `MediaMetadataDocument.metadata.dimensions` (falling back to `ImageDimensions` stored in fingerprint).
- In **Case A** (`primary_master_candidates.len() == 1`):
  - Check whether any external/staging candidate has strictly higher resolution (or equal resolution and larger file size).
  - If a superior candidate exists outside master:
    - Superior candidate is marked `NeedsIngestToMaster`.
    - Inferior master candidate remains on disk (protected from programmatic deletion) but is flagged for replacement in the manual deletion manifest.
    - Inferior external candidates are marked `MarkForRemoval`.
- In **Case B** (`primary_master_candidates.len() == 0`):
  - Candidates are sorted primarily by `MediaQualityRank` (pixel count descending, file size descending), then by drive group priority.
  - The highest-quality candidate becomes `NeedsIngestToMaster`.
  - All lower-quality duplicates become `MarkForRemoval`.

### Rationale
- Completely fulfills the user requirement: when deduplicating, lower resolution or small files are systematically marked for removal, while the highest resolution copy is guaranteed to be preserved and ingested into master.

### Alternatives Considered
- *Manual Conflict Review for all size mismatches*: Rejected as too noisy; operators expect automated pipelines to keep the best quality image without stopping every time a thumbnail or preview is encountered.

---

## 3. Backward Compatibility & Safe Fallbacks

### Decision
- When dimension metadata is missing (e.g. audio, corrupt headers, unsupported container formats), `MediaQualityRank` treats pixel count as `None` (0) and cleanly falls back to file size in bytes and existing drive priority.
- Zero breaking schema changes to MongoDB: `MediaMetadataDocument` already contains `metadata.dimensions` and `file_context.file_size_bytes`.
