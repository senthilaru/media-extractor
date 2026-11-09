# Contract: MongoDB Storage Sink CLI Options & Wire Specification

**Feature**: `005-mongodb-storage-sink`  
**Date**: 2026-11-08  
**Status**: Ready  

---

## 1. CLI Invocation Interface

```text
metadata-extractor [OPTIONS] <PATH>
```

### Positional Arguments
- `<PATH>` *(Mandatory)*: Directory path to recursively scan or single media file path.

### MongoDB Sink Options
- `--mongodb-uri <URI>` (alias: `--db-uri`):
  MongoDB connection URI string.
  *Example*: `mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db?authSource=media_index_db`
- `--collection <NAME>` (alias: `--db-collection`):
  Target collection name within the database.
  *Default*: `media_assets`

### Existing Complementary Flags
- `-o, --output <FILE>`: Destination file to concurrently save NDJSON output while persisting to MongoDB.
- `-p, --pretty [BOOL]`: Indent output JSON (when stdout/file output is enabled).
- `-q, --quiet`: Suppress live scan and ingestion heartbeats on `stderr`.
- `-b, --batch-size <NUM>`: Batch size for discovery, extraction, and bulk database writes (default: `500`).
- `-c, --channel-capacity <NUM>`: Bounded queue capacity for backpressure control (default: `100`).

---

## 2. Ingestion Telemetry Specification (stderr)

When `--mongodb-uri` is specified and `--quiet` is not set, the live telemetry heartbeat reports database write progress:

```text
[Scan & Ingest] Visited: 1240 | Found: 4500 | DB Ingested: 4500 (New: 4480, Updated: 20) | Rate: 1250.0 fps | DB Errors: 0
```

Upon scan completion:
```text
Scan & Ingest completed for /Volumes/SP-1/DCIM.
  Total Scanned: 4,500
  Database Ingested: 4,500 (4,480 inserted, 20 updated, 0 failed)
  Throughput: 1,280.5 docs/sec
  Session ID: 9f82d1c4-72b1-4f1b-8521-bb1c902d512a
```

---

## 3. Exit Codes & Error Scenarios

| Exit Code | Condition | Behavior |
|:---:|:---|:---|
| `0` | Success | All media items discovered and persisted; connection closed cleanly. |
| `1` | Traversal or extraction fatal error | Unrecoverable filesystem error. |
| `2` | Database Connection or Authentication Failure | Fail-fast check during startup fails (e.g. invalid URI, bad credentials, unreachable host). Traversal is NOT started. |
| `130` | Interrupted (SIGINT / Ctrl+C) | In-flight batch is flushed, connection pool is closed, and scanner exits cleanly. |

---

## 4. Connection String Sanitization Contract

Whenever connection URIs are logged or displayed in errors, passwords MUST be masked:
- Raw URI: `mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db?authSource=media_index_db`
- Sanitized URI: `mongodb://media_app:***@localhost:27017/media_index_db?authSource=media_index_db`
