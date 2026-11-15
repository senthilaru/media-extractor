# Phase 1 Data Model: Ingest Nikon NEF Raw Format

**Feature**: Ingest Nikon NEF Raw Format  
**Branch**: `022-support-nef-ingestion`  
**Date**: 2026-11-15  

## Entities & Models

### 1. `MediaMetadataDocument` (Enhanced Field Rules)

| Field | Type | Rules & Constraints |
| :--- | :--- | :--- |
| `id` | `Uuid` | Unique identifier for the catalog entry. |
| `file_context.file_path` | `PathBuf` | Absolute path to the media file on disk. |
| `file_context.extension` | `String` | Lowercase extension (`nef`). |
| `file_context.media_category` | `MediaCategory` | Must evaluate to `MediaCategory::RawPhoto`. |
| `dimensions` | `Option<ImageDimensions>` | Must record the full-resolution sensor dimensions (`width >= 1000`, `height >= 1000`) rather than thumbnail dimensions. |
| `temporal.timestamp_utc` | `DateTime<Utc>` | Parsed from `ExifDateTimeOriginal` (falling back to filesystem modified time if missing). |
| `temporal.source` | `TimestampSource` | `ExifDateTimeOriginal` or `FileSystemModified`. |
| `camera` | `Option<CameraMetadata>` | Make (`NIKON CORPORATION`), model (`NIKON D7100`, etc.). |
| `exposure` | `Option<ExposureMetadata>` | ISO, aperture, exposure time, focal length. |
| `fingerprint` | `Option<MediaFingerprint>` | Mandatory for deduplication: contains BLAKE3 cryptographic hash and sample digest. |
| `extraction_status.status` | `ExtractionResultKind` | `Success`, `Partial`, or `Failed`. |
| `dedup` | `Option<DeduplicationMetadata>` | Ingestion and deduplication lifecycle status. |

### 2. `DeduplicationMetadata` (State Lifecycle for NEF)

```
[Discovered on Source Drive]
          │
          ▼
   (Extraction)
          │
          ▼
  [dedup.status = None]
          │
          ▼
   (Deduplication Engine)
          │
          ├────────────────────────────────────────┐
          │ (Unique or superior)                   │ (Exact bitwise duplicate)
          ▼                                        ▼
[NeedsIngestToMaster]                     [MarkForRemoval]
          │
          ▼
   (Ingest Service)
          │
          ▼
  [CanonicalMaster]
  Path: <MasterRoot>/<Year>/<DD-Mon-YYYY>/PhotosRaw/<filename>
          │
          ▼
(Replica Sync Service)
          │
          ▼
  [ReplicaKeep] on Backup Drives (SP-2)
```

### 3. Incremental Pre-Filter Entity

| Field | Source | Extraction Decision Rule |
| :--- | :--- | :--- |
| `file_size_bytes` | Filesystem | If mismatched with DB -> Re-extract. |
| `modified_at` | Filesystem | If mismatched with DB -> Re-extract. |
| `extraction_status.status` | MongoDB | If `Failed` -> **Re-extract** (self-healing). |
| `fingerprint.cryptographic_hash` | MongoDB | If missing / null -> **Re-extract** (self-healing). |
| All matching & healthy | Both | Skip extraction (fast incremental path). |
