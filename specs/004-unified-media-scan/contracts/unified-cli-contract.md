# Contract: Unified CLI & MediaMetadataDocument JSON Schema

**Feature**: `004-unified-media-scan`  
**Date**: 2026-11-07  
**Status**: Ready for Planning  

---

## 1. CLI Invocation Interface

```text
metadata-extractor [OPTIONS] <PATH>
```

### Positional Arguments
- `<PATH>` *(Mandatory)*: The path to a single media file or a root directory to recursively scan and extract.

### Optional Flags
- `-o, --output <FILE>`: File path to write extracted metadata records (defaults to `stdout`).
- `-p, --pretty [BOOL]`: Format JSON with pretty indentation (defaults to `false` for directories, `true` for single files).
- `-q, --quiet`: Mute real-time scan telemetry and progress heartbeats on `stderr`.
- `-b, --batch-size <NUM>`: Number of discovered media items per channel batch (default: `500`).
- `-c, --channel-capacity <NUM>`: Maximum batches buffered before applying backpressure (default: `100`).
- `--include-hidden`: Include hidden files and directories starting with `.` (default: `false`).
- `--follow-symlinks`: Follow directory symlinks with loop detection (default: `false`).

---

## 2. Document JSON Schema (MongoDB Ready)

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "MediaMetadataDocument",
  "type": "object",
  "required": [
    "id",
    "file_context",
    "temporal",
    "extraction_status"
  ],
  "properties": {
    "id": {
      "type": "string",
      "format": "uuid",
      "description": "Unique UUID v4 record identifier"
    },
    "file_context": {
      "type": "object",
      "required": [
        "scanned_root",
        "file_path",
        "relative_path",
        "file_name",
        "file_size_bytes",
        "extension",
        "media_category"
      ],
      "properties": {
        "scanned_root": { "type": "string" },
        "file_path": { "type": "string" },
        "relative_path": { "type": "string" },
        "file_name": { "type": "string" },
        "file_size_bytes": { "type": "integer", "minimum": 0 },
        "extension": { "type": "string" },
        "media_category": { "type": "string", "enum": ["Photo", "Video", "RawPhoto"] }
      }
    },
    "temporal": {
      "type": "object",
      "required": [
        "iso_timestamp",
        "year",
        "month",
        "day",
        "hour",
        "minute",
        "second",
        "source"
      ],
      "properties": {
        "timestamp_utc": { "type": ["string", "null"], "format": "date-time" },
        "iso_timestamp": { "type": "string" },
        "year": { "type": "integer" },
        "month": { "type": "integer", "minimum": 1, "maximum": 12 },
        "day": { "type": "integer", "minimum": 1, "maximum": 31 },
        "hour": { "type": "integer", "minimum": 0, "maximum": 23 },
        "minute": { "type": "integer", "minimum": 0, "maximum": 59 },
        "second": { "type": "integer", "minimum": 0, "maximum": 59 },
        "source": { "type": "string" }
      }
    },
    "camera": {
      "type": ["object", "null"],
      "properties": {
        "make": { "type": ["string", "null"] },
        "model": { "type": ["string", "null"] },
        "lens_model": { "type": ["string", "null"] },
        "software": { "type": ["string", "null"] }
      }
    },
    "exposure": {
      "type": ["object", "null"],
      "properties": {
        "f_number": { "type": ["number", "null"] },
        "exposure_time_seconds": { "type": ["number", "null"] },
        "exposure_time_str": { "type": ["string", "null"] },
        "iso": { "type": ["integer", "null"] },
        "focal_length_mm": { "type": ["number", "null"] },
        "flash_fired": { "type": ["boolean", "null"] }
      }
    },
    "location": {
      "type": ["object", "null"],
      "properties": {
        "latitude": { "type": "number" },
        "longitude": { "type": "number" },
        "altitude_meters": { "type": ["number", "null"] }
      }
    },
    "dimensions": {
      "type": ["object", "null"],
      "properties": {
        "width": { "type": "integer" },
        "height": { "type": "integer" },
        "orientation": { "type": ["integer", "null"] }
      }
    },
    "video": {
      "type": ["object", "null"],
      "properties": {
        "duration_ms": { "type": "integer" },
        "video_codec": { "type": "string" },
        "audio_codec": { "type": ["string", "null"] },
        "frame_rate_fps": { "type": "number" },
        "rotation_degrees": { "type": "integer" }
      }
    },
    "fingerprint": {
      "type": ["object", "null"],
      "required": ["cryptographic_hash", "is_low_entropy"],
      "properties": {
        "cryptographic_hash": { "type": "string" },
        "sample_digest": { "type": ["string", "null"] },
        "phash": { "type": ["integer", "null"] },
        "phash_hex": { "type": ["string", "null"] },
        "rotated_hashes": {
          "type": ["object", "null"],
          "properties": {
            "hash_90": { "type": "integer" },
            "hash_180": { "type": "integer" },
            "hash_270": { "type": "integer" }
          }
        },
        "is_low_entropy": { "type": "boolean" }
      }
    },
    "extraction_status": {
      "type": "object",
      "required": ["status", "warnings"],
      "properties": {
        "status": { "type": "string", "enum": ["Success", "Partial", "Failed"] },
        "warnings": { "type": "array", "items": { "type": "string" } },
        "error_message": { "type": ["string", "null"] }
      }
    }
  }
}
```
