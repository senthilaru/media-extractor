# Contract: Nikon NEF RAW Ingestion & Lifecycle

**Feature**: Ingest Nikon NEF Raw Format  
**Branch**: `022-support-nef-ingestion`  
**Date**: 2026-11-15  

## 1. CLI Commands

### 1.1 Single File Extraction
```bash
cargo run -- extract <PATH_TO_NEF>
```
- **Contract Output**: Valid JSON metadata document.
- **Contract Assertions**:
  - `file_context.media_category == "RawPhoto"`
  - `file_context.extension == "nef"`
  - `dimensions.width >= 1000` (true sensor width, not thumbnail 160)
  - `dimensions.height >= 1000` (true sensor height, not thumbnail 120)
  - `fingerprint.cryptographic_hash` is present (64-char BLAKE3 hex string).

### 1.2 Pipeline Organization
```bash
cargo run -- organize --source <SOURCE_DIR> -c drives.yaml
```
- **Contract Behavior**:
  - Stage 1: Extracts `.NEF` files from `<SOURCE_DIR>`.
  - Stage 2: Audits catalog; assigns `NeedsIngestToMaster` to unmastered NEF files.
  - Stage 3: Ingests NEF files to target master `<PrimaryMaster>/<Year>/<DD-Mon-YYYY>/PhotosRaw/<filename>`.
  - Stage 4: Synchronizes NEF files to replica backup drives (`SP-2`).
  - Stage 5: Safely prunes original source NEF files after bitwise verification.

## 2. Ingestion Path Structure Contract

```text
/Volumes/SP-1/SPSA/media/master/
└── 2019/
    └── 25-Oct-2019/
        ├── Photos/
        │   └── SPS_1127.JPG           <-- Standard JPEG photos
        └── PhotosRaw/
            ├── SPS_1127.NEF           <-- Full-resolution Nikon RAW
            └── SPS_1128.NEF
```
