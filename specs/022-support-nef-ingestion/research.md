# Phase 0 Research: Ingest Nikon NEF Raw Format

**Feature**: Ingest Nikon NEF Raw Format  
**Branch**: `022-support-nef-ingestion`  
**Date**: 2026-11-15  

## Research Questions & Decisions

### 1. Nikon NEF Container Structure & Dimension Extraction

- **Problem**: In Nikon `.NEF` files (which adhere to TIFF/EP container standards), `IFD0` specifies `ImageWidth` and `ImageHeight` as the embedded JPEG thumbnail preview (e.g. 160x120 pixels). Standard EXIF extractors (`nom-exif`) read `IFD0` first, recording 160x120 instead of the true 24MP raw sensor resolution (e.g. 6036x4020 or 6000x4000).
- **Investigation**: Inspected TIFF byte structures of real Nikon NEF files. `IFD0` contains tag `0x014A` (`SubIFDs`), which lists offsets to sub-IFDs. `SubIFD1` contains the full sensor dimensions:
  - `0x0100` (`ImageWidth`): 6036
  - `0x0101` (`ImageLength` / `Height`): 4020
- **Decision**: Implement a dedicated RAW TIFF dimension reader in `exif_parser.rs`. If a file is a RAW format (such as `.nef`), or if the primary extracted dimensions are thumbnail-scale ($\le 320$ pixels) while the file size is $> 1$ MB, inspect TIFF `SubIFDs` to extract the true sensor width and height.
- **Alternatives Considered**:
  - *External CLI (`exiftool` / `sips`)*: Prohibited by Constitution Principle II & IV (must not rely on external non-Rust CLI tools in core pipeline).
  - *Rust `image` crate decoding*: `image` crate does not support proprietary Bayer raw containers and fails to decode `.NEF`.
  - *Custom TIFF SubIFD traversal*: Clean, zero-cost, memory-safe Rust using existing buffered I/O, parsing only the TIFF IFD directory table without loading the multi-megabyte image payload into memory.

### 2. Resilient Incremental Filtering for Incomplete / Failed Records

- **Problem**: When a process is aborted (e.g. via Ctrl+C) or an external drive is temporarily unavailable, files are recorded in MongoDB with `extraction_status.status: "Failed"` or without a `fingerprint`. Currently, `filter_unmodified_items` only compares `file_size_bytes` and `modified_at`. If both match, it marks the file as "unmodified" and skips extraction, causing those files to remain permanently un-extracted, un-fingerprinted, and un-ingested.
- **Decision**: Update `filter_unmodified_items` in `MongoDbStorageSink` to also project `extraction_status.status` and `fingerprint.cryptographic_hash`. If a document's status is `Failed` or its cryptographic hash is missing, exclude it from `unmodified_items` so that it is re-queued for extraction and fingerprinting.
- **Alternatives Considered**:
  - *Requiring `--force` on the entire drive*: Wastes 30+ minutes re-reading 38,000+ healthy records over USB just to fix 100 failed files.
  - *Manual database cleanup*: High friction for end users. Automatic self-healing is superior.

### 3. Ingestion Target Hierarchy & Companion Protection

- **Decision**:
  - Target directory for `MediaCategory::RawPhoto` is `<MasterRoot>/<Year>/<DD-Mon-YYYY>/PhotosRaw/<filename>`.
  - Same-stem companions (e.g., `DSC_0001.NEF` and `DSC_0001.JPG`) are recognized as distinct media categories (`RawPhoto` vs `Photo`). Bitwise deduplication evaluates them independently because their cryptographic hashes are different. They are organized into `PhotosRaw/` and `Photos/` respectively.
  - Replica synchronization mirrors `PhotosRaw/` directly to backup drives (`SP-2`).

### 4. Hardware & Concurrency Bounds (Constitution Principle I & III)

- Reading NEF metadata only parses the TIFF header and IFD table (first ~250 KB), avoiding full-file buffering.
- Full cryptographic hashing streams in bounded 8 MB chunks via `UnifiedFileTransferService`.
- Progress tracking and timing metrics remain instrumented on all operations.
