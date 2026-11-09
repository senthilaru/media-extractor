# Data Model: Media Metadata Extraction Engine

**Feature**: `002-extract-media-metadata`  
**Date**: 2026-11-07  
**Status**: Completed  

---

## 1. Domain Entities & Value Objects

```mermaid
classDiagram
    direction TB
    class MediaMetadataDocument {
        +Uuid id
        +FileContext file_context
        +TemporalMetadata temporal
        +Option~CameraMetadata~ camera
        +Option~ExposureMetadata~ exposure
        +Option~GeoLocation~ location
        +Option~ImageDimensions~ dimensions
        +Option~VideoMetadata~ video
        +ExtractionStatus extraction_status
    }

    class FileContext {
        +PathBuf scanned_root
        +PathBuf file_path
        +PathBuf relative_path
        +String file_name
        +u64 file_size_bytes
        +String extension
        +MediaCategory media_category
    }

    class TemporalMetadata {
        +Option~DateTime~Utc~~ timestamp_utc
        +String iso_timestamp
        +i32 year
        +u32 month
        +u32 day
        +u32 hour
        +u32 minute
        +u32 second
        +TimestampSource source
    }

    class TimestampSource {
        <<enumeration>>
        ExifDateTimeOriginal
        ExifCreateDate
        QuickTimeCreationTime
        FileSystemModified
    }

    class CameraMetadata {
        +Option~String~ make
        +Option~String~ model
        +Option~String~ lens_model
        +Option~String~ software
    }

    class ExposureMetadata {
        +Option~f64~ focal_length_mm
        +Option~f64~ focal_length_35mm
        +Option~f64~ f_number
        +Option~f64~ exposure_time_seconds
        +Option~String~ exposure_time_str
        +Option~u32~ iso
        +Option~bool~ flash_fired
    }

    class GeoLocation {
        +f64 latitude
        +f64 longitude
        +Option~f64~ altitude_meters
    }

    class ImageDimensions {
        +u32 width
        +u32 height
        +Option~u16~ orientation
    }

    class VideoMetadata {
        +u64 duration_ms
        +Option~String~ video_codec
        +Option~String~ audio_codec
        +Option~f64~ frame_rate_fps
        +Option~u64~ bitrate_kbps
        +Option~i32~ rotation_degrees
    }

    class ExtractionStatus {
        +ExtractionResultKind status
        +List~String~ warnings
        +Option~String~ error_message
    }

    class ExtractionResultKind {
        <<enumeration>>
        Success
        Partial
        Failed
    }

    MediaMetadataDocument "1" *-- "1" FileContext
    MediaMetadataDocument "1" *-- "1" TemporalMetadata
    MediaMetadataDocument "1" *-- "0..1" CameraMetadata
    MediaMetadataDocument "1" *-- "0..1" ExposureMetadata
    MediaMetadataDocument "1" *-- "0..1" GeoLocation
    MediaMetadataDocument "1" *-- "0..1" ImageDimensions
    MediaMetadataDocument "1" *-- "0..1" VideoMetadata
    MediaMetadataDocument "1" *-- "1" ExtractionStatus
    TemporalMetadata "1" *-- "1" TimestampSource
    ExtractionStatus "1" *-- "1" ExtractionResultKind
```

---

## 2. Entity Specifications

### 2.1 `MediaMetadataDocument` (Aggregate Root)
The primary normalized document representing an analyzed media file on disk. Designed for direct insertion into document stores like MongoDB.

| Field | Type | Required | Description |
|---|---|---|---|
| `id` | `Uuid` | Yes | Unique identifier for this media document |
| `file_context` | `FileContext` | Yes | Filesystem and directory hierarchy attributes |
| `temporal` | `TemporalMetadata` | Yes | Extracted capture datetime and derived calendar facets |
| `camera` | `Option<CameraMetadata>` | No | Hardware details (make, model, lens, software) |
| `exposure` | `Option<ExposureMetadata>` | No | Optical settings (shutter, aperture, ISO, focal length) |
| `location` | `Option<GeoLocation>` | No | GPS coordinates in decimal degrees |
| `dimensions` | `Option<ImageDimensions>` | No | Pixel width, height, and EXIF orientation |
| `video` | `Option<VideoMetadata>` | No | Video container specifics (duration, codec, fps, rotation) |
| `extraction_status` | `ExtractionStatus` | Yes | Success/partial status and diagnostic warnings |

---

### 2.2 `FileContext` (Value Object)
Captures the spatial context of the file relative to the scan root (e.g. `/Volumes/SP-1`).

| Field | Type | Description | Validation |
|---|---|---|---|
| `scanned_root` | `PathBuf` | The root folder supplied to the scanner (e.g. `/Volumes/SP-1`) | Non-empty path |
| `file_path` | `PathBuf` | Full canonical path on disk | Absolute path |
| `relative_path` | `PathBuf` | Path relative to `scanned_root` | Non-empty relative path |
| `file_name` | `String` | Base file name with extension (e.g. `IMG_1042.CR3`) | Non-empty string |
| `file_size_bytes` | `u64` | Byte size reported by filesystem | $\ge 0$ |
| `extension` | `String` | Normalized lowercase extension | Non-empty string |
| `media_category` | `MediaCategory` | Category (`Photo`, `Video`, `RawPhoto`) | Enum value |

---

### 2.3 `TemporalMetadata` (Value Object)
Decomposes capture timestamps into queryable integer facets optimized for database indexes.

| Field | Type | Description | Example / Range |
|---|---|---|---|
| `timestamp_utc` | `Option<DateTime<Utc>>` | UTC capture instant | `2026-10-14T08:12:45Z` |
| `iso_timestamp` | `String` | ISO 8601 formatted timestamp string | `"2026-10-14T08:12:45Z"` |
| `year` | `i32` | Integer year facet for database partitioning | `2026` |
| `month` | `u32` | Integer month facet | `1..=12` |
| `day` | `u32` | Integer day facet | `1..=31` |
| `hour` | `u32` | Integer hour facet (24h) | `0..=23` |
| `minute` | `u32` | Integer minute facet | `0..=59` |
| `second` | `u32` | Integer second facet | `0..=59` |
| `source` | `TimestampSource` | Origin tag of the timestamp | Enum value |

---

### 2.4 `GeoLocation` (Value Object)
GPS coordinates converted from EXIF rational degrees/minutes/seconds into signed decimal degrees.

| Field | Type | Range | Description |
|---|---|---|---|
| `latitude` | `f64` | `[-90.0, 90.0]` | North (+) or South (-) decimal degrees |
| `longitude` | `f64` | `[-180.0, 180.0]` | East (+) or West (-) decimal degrees |
| `altitude_meters` | `Option<f64>` | Any | Altitude above/below sea level |

---

## 3. Invariants & Data Integrity Rules

1. **Deterministic Calendar Facets**: `year`, `month`, and `day` MUST strictly correspond to `iso_timestamp` in UTC.
2. **Fallback Integrity**: If embedded EXIF or video timestamps are missing, the system MUST compute temporal facets from the file's modification time and set `source = TimestampSource::FileSystemModified`.
3. **Lossless JSON Serialization**: All numeric types MUST serialize to standard JSON numbers (integers or floating-point); non-finite floats (`NaN`, `Infinity`) MUST be serialized as `null`.
4. **Fault Tolerance**: Incomplete, corrupt, or truncated headers MUST produce a `MediaMetadataDocument` with `extraction_status.status = ExtractionResultKind::Partial` or `Failed`, preserving all valid `file_context` attributes.
