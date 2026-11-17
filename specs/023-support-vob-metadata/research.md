# Research: Support for VOB Video Extensions and DVD Metadata Files

## Technical Decisions

### 1. Classification & Category Mapping

- **Decision**:
  - Classify `.vob` (case-insensitive) as `MediaCategory::Video`.
  - Classify `.ifo` and `.bup` (case-insensitive) as `MediaCategory::Sidecar`.
- **Rationale**:
  - `.vob` (Video Object) contains multiplexed MPEG-2 video, AC-3 / DTS / LPCM audio, and subtitles. It represents playable video content.
  - `.ifo` (InFormation) and `.bup` (Backup) contain DVD playback control, title sets, chapter points, and stream language mapping. They are non-playable metadata sidecars that accompany `.vob` streams.
  - Reusing `MediaCategory::Sidecar` preserves the existing architecture established for sidecar files without requiring a new top-level category enum.
- **Alternatives Considered**:
  - Creating a separate `MediaCategory::DvdMetadata`: Rejected as unnecessary complexity. Treating `.ifo` and `.bup` as `MediaCategory::Sidecar` allows existing sidecar co-location, replication, and pruning logic to apply naturally.

### 2. DVD Video Metadata Extraction

- **Decision**:
  - Route `.vob` through `VideoMetadataExtractor`.
  - Provide fallback codec identification (`mpeg2` for video, `ac3/lpcm/dts` for audio) and fall back to filesystem modification time when container headers lack QuickTime creation atoms.
- **Rationale**:
  - Standard `.vob` files are MPEG-2 Program Streams rather than ISO-BMFF / MP4 containers.
  - `nom-exif` and `inspect_container_atoms` handle QuickTime and MP4 atoms; for non-atom MPEG-2 streams, graceful fallback ensures the file is indexed, hashed with BLAKE3, and organized without errors or warnings.
- **Alternatives Considered**:
  - Integrating `libavformat` or `ffmpeg`: Rejected to prevent heavy external C/C++ dependencies and stay within pure Rust bounds.

### 3. Companion Sidecar Stem Matching

- **Decision**:
  - Generalize companion sidecar matching to support both Apple edit conventions and DVD Video Title Set conventions:
    - **Exact Match**: `media_stem.eq_ignore_ascii_case(sidecar_stem)` (e.g. `VTS_01_1.VOB` and `VTS_01_1.IFO`).
    - **DVD Title Set Root Match**: If `media_stem` matches `VTS_xx_*` and `sidecar_stem` matches `VTS_xx_0` (where `xx` is the 2-digit title set number, e.g. `VTS_01`), they are recognized as companions.
    - **DVD Disc Root Match**: `VIDEO_TS.VOB` matches `VIDEO_TS.IFO` and `VIDEO_TS.BUP`.
    - **Apple Conventions**: Retain existing `IMG_E*`, `IMG_O*`, `OIMG_*` matching for `.aae`.
- **Rationale**:
  - DVD folder specifications (`VIDEO_TS`) define title set streams as `VTS_xx_1.VOB`, `VTS_xx_2.VOB` sharing navigation metadata in `VTS_xx_0.IFO` and `VTS_xx_0.BUP`. Supporting this pattern guarantees metadata is co-located with the video.
- **Alternatives Considered**:
  - Requiring strict 1:1 filename matching only: Rejected because real-world DVDs use `VTS_01_0.IFO` for `VTS_01_1.VOB`.

### 4. Multiple Companion Sidecar Support

- **Decision**:
  - Support transferring and verifying multiple companion sidecars (e.g., both `.IFO` and `.BUP`) associated with a single media asset.
  - Update `UnifiedFileTransferService::find_companion_sidecars` to return `Vec<PathBuf>` and `transfer_companion_sidecars_if_present` to transfer all detected companion files.
- **Rationale**:
  - DVDs always include pairs of `.IFO` and `.BUP` files. A single-element lookup would leave `.BUP` files behind.
