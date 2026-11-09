# Quickstart & Validation Guide: Direct MongoDB Storage Sink

**Feature**: `005-mongodb-storage-sink`  
**Date**: 2026-11-08  
**Status**: Ready  

---

## 1. Prerequisites

- **Rust Toolchain**: Rust 1.82+ with `cargo`.
- **Local MongoDB**: MongoDB 4.4+ running locally (e.g., on `localhost:27017` or via Docker):
  ```bash
  # Example: Starting a local MongoDB container with authentication
  docker run -d --name local-mongo -p 27017:27017 \
    -e MONGO_INITDB_ROOT_USERNAME=media_app \
    -e MONGO_INITDB_ROOT_PASSWORD=<MONGODB_PASSWORD> \
    -e MONGO_INITDB_DATABASE=media_index_db \
    mongo:7.0
  ```

---

## 2. Compilation

Compile the project in release mode:

```bash
cargo build --release
```

The compiled binary is available at `./target/release/metadata-extractor`.

---

## 3. End-to-End Validation Scenarios

### Scenario 1: Direct Recursive Scan into Local MongoDB
Scan a directory of photos, videos, and camera RAW assets directly into the `media_assets` collection:

```bash
./target/release/metadata-extractor tests/fixtures/valid_samples \
  --mongodb-uri "mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db?authSource=media_index_db"
```

**Expected Outcome**:
- Verifies MongoDB connection and creates default indexes.
- Recursively discovers all media assets.
- Ingests all extracted metadata documents directly into the `media_assets` collection.
- Displays live ingestion progress on `stderr`.

**Verification via `mongosh`**:
```javascript
// Connect to MongoDB
mongosh "mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db?authSource=media_index_db"

// Check collection count (should match discovered files count)
db.media_assets.countDocuments();

// Inspect first document
db.media_assets.findOne({}, { "file_context.file_name": 1, "fingerprint.cryptographic_hash": 1, "temporal.year": 1 });
```

---

### Scenario 2: Idempotent Re-Scan (Zero Duplicates)
Re-run the exact same command on the same directory:

```bash
./target/release/metadata-extractor tests/fixtures/valid_samples \
  --mongodb-uri "mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db?authSource=media_index_db"
```

**Expected Outcome**:
- In `mongosh`, `db.media_assets.countDocuments()` is identical to the first run.
- Telemetry indicates 0 new inserts and 100% updated/verified records.
- No duplicate key errors occur.

---

### Scenario 3: Fail-Fast Bad Credential Validation
Execute the scan with an invalid password:

```bash
./target/release/metadata-extractor tests/fixtures/valid_samples \
  --mongodb-uri "mongodb://media_app:wrong_password@localhost:27017/media_index_db?authSource=media_index_db"
```

**Expected Outcome**:
- Command fails immediately (within 2 seconds) with a clear authentication failure error.
- Traversal is NOT initiated.
- The password is masked in output: `mongodb://media_app:***@localhost:27017/...`.

---

### Scenario 4: Concurrent Ingestion & Local File Backup
Persist directly to MongoDB while simultaneously saving an NDJSON backup file:

```bash
./target/release/metadata-extractor tests/fixtures/valid_samples \
  --mongodb-uri "mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db?authSource=media_index_db" \
  -o ./backup_catalog.ndjson
```

**Expected Outcome**:
- Documents are saved to MongoDB.
- `./backup_catalog.ndjson` is populated with the matching NDJSON records.
