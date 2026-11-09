# Feature Specification: Perceptual Fingerprinting & Hash Extraction Engine

**Feature Branch**: `003-perceptual-deduplication`  
**Created**: 2026-11-07  
**Updated**: 2026-11-07  
**Status**: Revised (Scope Simplified per User Request)  
**Input**: User description: "Extract perceptual hashes (phash) and fingerprints to identify identical or near-duplicate media assets, just add the required hash in the JSON, don't want to scan or mark or reference any duplicate files"

---

## Scope & Intent

The system extracts cryptographic digests and perceptual visual hashes (pHash) on individual media assets and embeds the resulting `fingerprint` object directly into each file's normalized metadata JSON document (`MediaMetadataDocument`). 

**Explicitly Out of Scope**:
- Cross-file scanning or pairwise comparisons to detect duplicate pairs.
- Forming duplicate clusters (`DuplicateCluster`).
- Selecting canonical masters vs duplicate variants (`CanonicalAsset`, `DuplicateVariant`).
- Marking, symlinking, purging, or referencing duplicate files.
- Computing dataset-wide storage savings or reclaimable bytes.

Downstream systems and databases (e.g., MongoDB, Elasticsearch) utilize the embedded hashes for querying identical files (`cryptographic_hash`) or near-duplicates (`phash` Hamming distance queries).

---

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Cryptographic Content Hashing (Priority: P1) 🎯 MVP

As a media library administrator or database engineer, I want the system to compute streaming cryptographic digests (BLAKE3) and fast sample digests for each media file and include them directly in the file's metadata JSON, so that downstream systems can instantly identify 100% bitwise identical assets via standard index queries without re-reading files.

**Why this priority**: Exact binary identity is fundamental to cataloging and integrity verification. Streaming BLAKE3 provides cryptographic security at multi-gigabyte/sec speeds without buffering entire files in memory.

**Independent Test**: Extract metadata for a directory of media files; verify each output JSON document contains a `fingerprint` section with a 64-hex character `cryptographic_hash` and a `sample_digest`, identical files share identical hashes, and memory usage remains strictly bounded (< 100 MB).

**Acceptance Scenarios**:
1. **Given** any valid photo, video, or RAW file, **When** metadata is extracted, **Then** the output JSON includes `fingerprint.cryptographic_hash` containing the 64-character lowercase hex BLAKE3 digest of the complete file payload.
2. **Given** two separate files with identical byte contents but different filenames or locations, **When** extracted, **Then** both documents emit the exact same `cryptographic_hash` value.
3. **Given** a large media file (> 1 GB), **When** cryptographic hashing is performed, **Then** the digest is computed in streaming 64 KB buffers with zero memory spikes.
4. **Given** a media file, **When** extracted, **Then** `fingerprint.sample_digest` contains the fast hash of header (first 16 KB) + trailer (last 16 KB) + file size.

---

### User Story 2 - Perceptual Visual Hash (pHash) Extraction (Priority: P2)

As a photo library curator or search architect, I want the system to compute 64-bit DCT perceptual visual hashes (pHash) on photos and camera RAW preview frames, compute orthogonal rotation hashes (90°, 180°, 270°), and detect low-entropy/blank frames, embedding these directly in the metadata JSON, so that downstream systems can identify resized, re-encoded, or rotated visual near-duplicates using simple Hamming distance queries.

**Why this priority**: Images frequently exist as re-compressed WebP copies, social media downloads, downscaled thumbnails, or rotated captures. Storing perceptual hashes in each file's record allows database queries to find visual clones without loading or decoding images during search.

**Independent Test**: Extract metadata for high-resolution originals, 50% downscaled thumbnails, and rotated variants; verify that all output documents contain `fingerprint.phash` (64-bit integer and hex string), rotated variant hashes, and `fingerprint.is_low_entropy` boolean flag. Verify that the Hamming distance between an original and thumbnail is $\le 10$ bits, and rotated hashes match 90°/180°/270° orientations.

**Acceptance Scenarios**:
1. **Given** a compressed photo (JPEG, PNG, WebP) or camera RAW file with embedded preview, **When** extracted, **Then** the output JSON includes `fingerprint.phash` as a 64-bit unsigned integer and `fingerprint.phash_hex` as a 16-character hexadecimal string.
2. **Given** an image and a downscaled or recompressed variant of the same image, **When** their pHashes are extracted, **Then** their bit difference (Hamming distance) is $\le 10$.
3. **Given** an image, **When** extracted, **Then** `fingerprint.rotated_hashes` contains `hash_90`, `hash_180`, and `hash_270` representing the DCT pHash for each orthogonal clockwise rotation.
4. **Given** a solid-color, blank, or monochromatic frame (e.g. black or white), **When** extracted, **Then** `fingerprint.is_low_entropy` is set to `true`, preventing false-positive near-duplicate queries downstream.
5. **Given** a video file, **When** extracted, **Then** `fingerprint.cryptographic_hash` is populated while `fingerprint.phash` is safely set to `null` if video preview decoding is skipped.

---

### User Story 3 - Integrated Metadata Document & JSON/NDJSON Emission (Priority: P3)

As a database administrator, I want the `fingerprint` object to be a standard, first-class field of the `MediaMetadataDocument` emitted by `extract` and `extract-stream`, so that a single unified command extracts EXIF, video atoms, calendar facets, and content fingerprints into a single MongoDB-ready JSON/NDJSON record per file without running separate tools or scanning passes.

**Why this priority**: Operating a unified pipeline saves disk I/O and CPU cycles. Downstream ingestion tools (like `mongoimport`) can stream the records directly into collections.

**Independent Test**: Run `metadata-extractor extract <file>` and verify the single pretty-printed JSON output includes all metadata fields and the `fingerprint` object. Run `metadata-extractor scan <dir> | metadata-extractor extract-stream` and verify each emitted NDJSON line contains the complete document with its `fingerprint`.

**Acceptance Scenarios**:
1. **Given** a single media file, **When** `metadata-extractor extract <path>` is executed, **Then** it prints a single JSON document containing `file_context`, `temporal`, `camera`, `exposure`, `dimensions`, and `fingerprint`.
2. **Given** a stream of discovered files from `metadata-extractor scan`, **When** piped into `metadata-extractor extract-stream`, **Then** it emits streaming NDJSON records where every line is a complete `MediaMetadataDocument` containing the `fingerprint` section.
3. **Given** a corrupted media file where image decoding fails, **When** extracted, **Then** `fingerprint.cryptographic_hash` is computed, `fingerprint.phash` is `null`, and the extraction error is recorded in `extraction_status` without crashing the pipeline.

---

### Edge Cases

- **Corrupted or Truncated Files**: Cryptographic hash is computed on available bytes if readable; if image decoding fails for pHash, `fingerprint.phash` is `null` and diagnostic recorded in `extraction_status`.
- **Zero-Byte Files**: Recorded with `extraction_status.status = "Failed"` and error `ZeroByteFile`; empty hash fields.
- **Monochromatic / Low-Entropy Images**: Flagged with `fingerprint.is_low_entropy = true` based on pixel variance ($\sigma^2 < 5.0$).
- **Camera RAW Files**: Embedded JPEG preview extracted for pHash calculation without expensive full sensor demosaicing.
- **Videos**: BLAKE3 cryptographic hash computed over full video container; `phash` is optional/null when visual keyframes are not decoded.

---

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: System MUST compute a full streaming BLAKE3 cryptographic digest (`cryptographic_hash`) for each media file in bounded 64 KB chunks without buffering full files in memory.
- **FR-002**: System MUST compute a fast sample digest (`sample_digest`) using BLAKE3 over the file header (first 16 KB), trailer (last 16 KB), and file size.
- **FR-003**: System MUST compute a 64-bit pure-Rust 2D DCT perceptual visual hash (`phash`) for all supported photo formats (JPEG, PNG, WebP) and camera RAW preview frames:
  - Resize image directly to a $32 \times 32$ 8-bit grayscale luminance matrix.
  - Compute 2D Discrete Cosine Transform (DCT) and extract the top-left $8 \times 8$ low-frequency coefficients (excluding DC).
  - Compute median of the 64 coefficients and binarize into a 64-bit integer (`phash`) and 16-character hex string (`phash_hex`).
- **FR-004**: System MUST compute rotated perceptual visual hashes for 90°, 180°, and 270° clockwise orientations (`rotated_hashes.hash_90`, `rotated_hashes.hash_180`, `rotated_hashes.hash_270`) directly from the rotated $32 \times 32$ matrix without re-reading the source file.
- **FR-005**: System MUST compute luminance pixel variance across the 1024 thumbnail pixels and set `is_low_entropy: true` if variance $< 5.0$ to flag blank/monochromatic frames.
- **FR-006**: System MUST embed the `fingerprint` object directly into `MediaMetadataDocument` as a first-class field.
- **FR-007**: System MUST NOT perform cross-file duplicate clustering, pairwise comparisons, canonical master selection, or duplicate file marking/referencing.
- **FR-008**: System MUST emit valid MongoDB-ready JSON and streaming NDJSON from the `extract` and `extract-stream` subcommands containing the complete metadata and fingerprint data.
- **FR-009**: System MUST isolate per-file hashing and decoding errors, logging structured diagnostic records in `extraction_status` without aborting the batch pipeline.

---

### Data Schema: `MediaFingerprint` / `ContentFingerprint`

```json
{
  "cryptographic_hash": "b4c2d8e4f1a2...",
  "sample_digest": "a9f8e7d6...",
  "phash": 1311768467294899695,
  "phash_hex": "123456789abcdef0",
  "rotated_hashes": {
    "hash_90": 24589238479238,
    "hash_180": 89457298374982,
    "hash_270": 19283749283749
  },
  "is_low_entropy": false
}
```

Embedded directly within `MediaMetadataDocument`:

```json
{
  "id": "e6f9a0c2-124b-4b13-9a3b-28f9d0c6418b",
  "file_context": {
    "scanned_root": "/Volumes/SP-1/DCIM",
    "file_path": "/Volumes/SP-1/DCIM/100CANON/IMG_1042.CR3",
    "relative_path": "100CANON/IMG_1042.CR3",
    "file_name": "IMG_1042.CR3",
    "file_size_bytes": 34819200,
    "extension": "cr3",
    "media_category": "RawPhoto"
  },
  "temporal": { ... },
  "camera": { ... },
  "exposure": { ... },
  "dimensions": { "width": 8192, "height": 5464, "orientation": 1 },
  "fingerprint": {
    "cryptographic_hash": "c3d9a1f4...",
    "sample_digest": "8a7b6c5d...",
    "phash": 9847291847192847,
    "phash_hex": "0023f89a12c4e567",
    "rotated_hashes": {
      "hash_90": 1284719284719284,
      "hash_180": 5719284719284719,
      "hash_270": 9182736451928374
    },
    "is_low_entropy": false
  },
  "extraction_status": {
    "status": "Complete",
    "warnings": [],
    "errors": []
  }
}
```

---

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of successfully extracted media files include a valid 64-hex character BLAKE3 `cryptographic_hash` and `sample_digest`.
- **SC-002**: Photos and camera RAW preview frames include a 64-bit DCT `phash` with Hamming distance $\le 10$ between originals and downscaled/recompressed variants.
- **SC-003**: Fingerprinting throughput achieves $\ge 100$ files per second across multi-core Rayon worker threads.
- **SC-004**: Memory usage remains strictly bounded under 100 MB during streaming extraction.
- **SC-005**: 100% of emitted documents conform to the normalized JSON schema ready for direct MongoDB ingestion.
