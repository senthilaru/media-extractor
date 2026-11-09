# Contract: Normalized Metadata JSON Schema (MongoDB Ready)

**Feature**: `002-extract-media-metadata`  
**Date**: 2026-11-07  
**Status**: Completed  

---

## 1. Top-Level JSON Document Structure

Each processed media file produces an independent, self-contained JSON document:

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
      "description": "Unique document identifier (UUID v4)"
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
        "scanned_root": { "type": "string", "example": "/Volumes/SP-1" },
        "file_path": { "type": "string", "example": "/Volumes/SP-1/DCIM/100CANON/IMG_1042.CR3" },
        "relative_path": { "type": "string", "example": "DCIM/100CANON/IMG_1042.CR3" },
        "file_name": { "type": "string", "example": "IMG_1042.CR3" },
        "file_size_bytes": { "type": "integer", "minimum": 0, "example": 34819200 },
        "extension": { "type": "string", "example": "cr3" },
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
        "iso_timestamp": { "type": "string", "format": "date-time", "example": "2026-10-14T08:12:45Z" },
        "year": { "type": "integer", "example": 2026 },
        "month": { "type": "integer", "minimum": 1, "maximum": 12, "example": 10 },
        "day": { "type": "integer", "minimum": 1, "maximum": 31, "example": 14 },
        "hour": { "type": "integer", "minimum": 0, "maximum": 23, "example": 8 },
        "minute": { "type": "integer", "minimum": 0, "maximum": 59, "example": 12 },
        "second": { "type": "integer", "minimum": 0, "maximum": 59, "example": 45 },
        "source": {
          "type": "string",
          "enum": ["ExifDateTimeOriginal", "ExifCreateDate", "QuickTimeCreationTime", "FileSystemModified"]
        }
      }
    },
    "camera": {
      "type": ["object", "null"],
      "properties": {
        "make": { "type": ["string", "null"], "example": "Canon" },
        "model": { "type": ["string", "null"], "example": "Canon EOS R5" },
        "lens_model": { "type": ["string", "null"], "example": "RF24-70mm F2.8 L IS USM" },
        "software": { "type": ["string", "null"], "example": "Firmware 1.8.1" }
      }
    },
    "exposure": {
      "type": ["object", "null"],
      "properties": {
        "focal_length_mm": { "type": ["number", "null"], "example": 50.0 },
        "focal_length_35mm": { "type": ["number", "null"], "example": 50.0 },
        "f_number": { "type": ["number", "null"], "example": 2.8 },
        "exposure_time_seconds": { "type": ["number", "null"], "example": 0.004 },
        "exposure_time_str": { "type": ["string", "null"], "example": "1/250" },
        "iso": { "type": ["integer", "null"], "example": 100 },
        "flash_fired": { "type": ["boolean", "null"], "example": false }
      }
    },
    "location": {
      "type": ["object", "null"],
      "properties": {
        "latitude": { "type": "number", "minimum": -90.0, "maximum": 90.0, "example": 37.7749 },
        "longitude": { "type": "number", "minimum": -180.0, "maximum": 180.0, "example": -122.4194 },
        "altitude_meters": { "type": ["number", "null"], "example": 14.5 }
      }
    },
    "dimensions": {
      "type": ["object", "null"],
      "properties": {
        "width": { "type": "integer", "example": 8192 },
        "height": { "type": "integer", "example": 5464 },
        "orientation": { "type": ["integer", "null"], "example": 1 }
      }
    },
    "video": {
      "type": ["object", "null"],
      "properties": {
        "duration_ms": { "type": "integer", "example": 45200 },
        "video_codec": { "type": ["string", "null"], "example": "h265" },
        "audio_codec": { "type": ["string", "null"], "example": "aac" },
        "frame_rate_fps": { "type": ["number", "null"], "example": 59.94 },
        "bitrate_kbps": { "type": ["integer", "null"], "example": 48500 },
        "rotation_degrees": { "type": ["integer", "null"], "example": 0 }
      }
    },
    "extraction_status": {
      "type": "object",
      "required": ["status", "warnings"],
      "properties": {
        "status": { "type": "string", "enum": ["Success", "Partial", "Failed"] },
        "warnings": {
          "type": "array",
          "items": { "type": "string" }
        },
        "error_message": { "type": ["string", "null"] }
      }
    }
  }
}
```

---

## 2. Example Normalized Document

```json
{
  "id": "e6f9a0c2-124b-4b13-9a3b-28f9d0c6418b",
  "file_context": {
    "scanned_root": "/Volumes/SP-1",
    "file_path": "/Volumes/SP-1/DCIM/100CANON/IMG_1042.CR3",
    "relative_path": "DCIM/100CANON/IMG_1042.CR3",
    "file_name": "IMG_1042.CR3",
    "file_size_bytes": 34819200,
    "extension": "cr3",
    "media_category": "RawPhoto"
  },
  "temporal": {
    "iso_timestamp": "2026-10-14T08:12:45Z",
    "year": 2026,
    "month": 10,
    "day": 14,
    "hour": 8,
    "minute": 12,
    "second": 45,
    "source": "ExifDateTimeOriginal"
  },
  "camera": {
    "make": "Canon",
    "model": "Canon EOS R5",
    "lens_model": "RF24-70mm F2.8 L IS USM",
    "software": "Firmware 1.8.1"
  },
  "exposure": {
    "focal_length_mm": 50.0,
    "focal_length_35mm": 50.0,
    "f_number": 2.8,
    "exposure_time_seconds": 0.004,
    "exposure_time_str": "1/250",
    "iso": 100,
    "flash_fired": false
  },
  "location": {
    "latitude": 37.774929,
    "longitude": -122.419416,
    "altitude_meters": 14.5
  },
  "dimensions": {
    "width": 8192,
    "height": 5464,
    "orientation": 1
  },
  "video": null,
  "extraction_status": {
    "status": "Success",
    "warnings": [],
    "error_message": null
  }
}
```
