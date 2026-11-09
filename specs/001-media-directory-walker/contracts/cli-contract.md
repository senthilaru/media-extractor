# Contract: CLI & Streaming Protocol

**Feature**: `001-media-directory-walker`  
**Date**: 2026-11-07  
**Status**: Completed  

---

## 1. CLI Command Specification

The scanner functionality can be invoked directly from the command line:

```bash
metadata-extractor scan [OPTIONS] <PATHS>...
```

### Arguments
- `<PATHS>...`: One or more directory paths to scan recursively (required).

### Options
- `-b, --batch-size <NUM>`: Number of discovered media items per emitted batch (default: `500`).
- `-c, --channel-capacity <NUM>`: Maximum batches buffered before applying backpressure (default: `100`).
- `--include-hidden`: Do not ignore hidden directories and files (starting with `.`).
- `--follow-symlinks`: Follow symbolic links to directories (guarded by cycle detection).
- `--format <text|json>`: Output format (default: `text`).
- `-q, --quiet`: Suppress live telemetry heartbeats on stderr.

---

## 2. JSON Streaming Protocol (`--format json`)

When `--format json` is enabled, the CLI emits newline-delimited JSON (NDJSON) messages on `stdout` for downstream ingestion pipelines:

### 2.1 Batch Message (`type: "batch"`)
```json
{
  "type": "batch",
  "session_id": "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d",
  "batch_sequence": 1,
  "count": 2,
  "created_at": "2026-11-07T14:52:00Z",
  "items": [
    {
      "path": "/Volumes/SP-1/DCIM/100CANON/IMG_1042.CR3",
      "file_size_bytes": 34819200,
      "modified_at": "2026-10-14T08:12:45Z",
      "category": "RawPhoto",
      "extension": "cr3"
    },
    {
      "path": "/Volumes/SP-1/DCIM/100CANON/VID_0012.MOV",
      "file_size_bytes": 104857600,
      "modified_at": "2026-10-14T08:15:10Z",
      "category": "Video",
      "extension": "mov"
    }
  ]
}
```

### 2.2 Telemetry Heartbeat (`type: "telemetry"`)
Emitted periodically to `stderr`:
```json
{
  "type": "telemetry",
  "session_id": "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d",
  "elapsed_seconds": 5.0,
  "directories_scanned": 1240,
  "files_inspected": 84200,
  "media_files_discovered": 62100,
  "throughput_fps": 16840.0,
  "queue_depth": 14,
  "errors_encountered": 2
}
```

### 2.3 Skip/Error Record (`type: "error"`)
Emitted to `stderr` or error stream:
```json
{
  "type": "error",
  "path": "/Volumes/SP-1/System Volume Information/.private",
  "kind": "PermissionDenied",
  "message": "Permission denied (os error 13)",
  "occurred_at": "2026-11-07T14:52:01Z"
}
```
