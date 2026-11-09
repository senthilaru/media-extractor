# Contract: Metadata Document with Fingerprint JSON Schema

**Feature**: `003-perceptual-deduplication`  
**Date**: 2026-11-07  
**Status**: Revised (Scope Simplified per User Request)  

---

## 1. Content Fingerprint JSON Schema (Embedded in MediaMetadataDocument)

```json
{
  "$schema": "https://json-schema.org/draft/2020-12/schema",
  "title": "ContentFingerprint",
  "type": "object",
  "required": [
    "cryptographic_hash",
    "is_low_entropy"
  ],
  "properties": {
    "cryptographic_hash": {
      "type": "string",
      "pattern": "^[0-9a-f]{64}$",
      "description": "BLAKE3 64-character lowercase hexadecimal cryptographic digest of file payload"
    },
    "sample_digest": {
      "type": ["string", "null"],
      "pattern": "^[0-9a-f]{64}$",
      "description": "BLAKE3 64-character hex fast sample digest (header + trailer + size)"
    },
    "phash": {
      "type": ["integer", "null"],
      "minimum": 0,
      "description": "64-bit unsigned integer DCT perceptual visual hash for canonical 0° orientation"
    },
    "phash_hex": {
      "type": ["string", "null"],
      "pattern": "^[0-9a-f]{16}$",
      "description": "16-character lowercase hex string representation of 64-bit DCT pHash"
    },
    "rotated_hashes": {
      "type": ["object", "null"],
      "properties": {
        "hash_90": { "type": "integer", "description": "DCT pHash for 90° clockwise rotation" },
        "hash_180": { "type": "integer", "description": "DCT pHash for 180° rotation" },
        "hash_270": { "type": "integer", "description": "DCT pHash for 270° clockwise rotation" }
      },
      "required": ["hash_90", "hash_180", "hash_270"]
    },
    "is_low_entropy": {
      "type": "boolean",
      "description": "True if image is blank, monochromatic, or degenerate (variance < 5.0)"
    }
  }
}
```

---

## 2. Example Normalized Metadata Document Output

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
  "temporal": {
    "iso_timestamp": "2026-10-14T08:12:45+00:00",
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
    "model": "Canon EOS R5"
  },
  "exposure": {
    "f_number": 2.8,
    "exposure_time": "1/250",
    "iso": 100,
    "focal_length": 70.0
  },
  "dimensions": {
    "width": 8192,
    "height": 5464,
    "orientation": 1
  },
  "fingerprint": {
    "cryptographic_hash": "2fbc3879a12c4de69810a9cf1847120394871239481723948172394871293847",
    "sample_digest": "9a8b7c6d5e4f3a2b1c0d9e8f7a6b5c4d3e2f1a0b9c8d7e6f5a4b3c2d1e0f9a8b",
    "phash": 1311768467294899695,
    "phash_hex": "123456789abcdef0",
    "rotated_hashes": {
      "hash_90": 24589238479238,
      "hash_180": 89457298374982,
      "hash_270": 19283749283749
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
