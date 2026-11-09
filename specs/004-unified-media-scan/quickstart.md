# Quickstart & Validation Guide: Unified Media Metadata Scanner

**Feature**: `004-unified-media-scan`  
**Date**: 2026-11-07  
**Status**: Ready for Planning  

---

## 1. Prerequisites

- **Rust Toolchain**: Rust 1.82+ with `cargo`.
- **Operating System**: macOS (Apple Silicon M1/M2/M3 or Intel) or Linux (x86_64, aarch64).

---

## 2. Setup & Compilation

Compile the project in release mode:

```bash
cargo build --release
```

The compiled binary is available at `./target/release/metadata-extractor`.

---

## 3. End-to-End Validation Scenarios

### Scenario 1: Single-Command Recursive Directory Extraction
Provide a directory path to discover all media files across all subdirectories and stream rich metadata with embedded fingerprints:

```bash
# Unified command with 1 mandatory argument
./target/release/metadata-extractor /Volumes/SP-1/DCIM

# Or via cargo
cargo run --release -- /Volumes/SP-1/DCIM
```

**Expected Outcome**:
- Recursively walks all folders in `/Volumes/SP-1/DCIM`.
- Discovers photos, videos, and camera RAW assets.
- Emits streaming newline-delimited JSON (NDJSON) to `stdout` containing EXIF, container atoms, calendar facets, BLAKE3 hashes, and DCT pHashes.
- Live scan progress is displayed on `stderr` without polluting standard output.

---

### Scenario 2: Direct File Output via `-o / --output`
Save extracted metadata directly to a catalog file without shell redirection:

```bash
./target/release/metadata-extractor /Volumes/SP-1/DCIM -o ./media_catalog.ndjson
```

**Expected Outcome**:
- Extracted NDJSON records are saved directly to `media_catalog.ndjson`.
- The file is immediately ready for database import.

---

### Scenario 3: Single File Metadata Extraction
Provide a single photo, video, or RAW file to display its complete formatted metadata:

```bash
./target/release/metadata-extractor /Volumes/SP-1/DCIM/100CANON/IMG_1042.CR3
```

**Expected Outcome**:
- Outputs a single, pretty-printed JSON document with complete file context, EXIF/exposure data, and fingerprint hashes.

---

### Scenario 4: Direct Import into MongoDB
Import the generated catalog into MongoDB with zero transformation:

```bash
mongoimport --uri "mongodb://localhost:27017/media_library" \
  --collection items \
  --file ./media_catalog.ndjson
```

**Verify in MongoDB Shell (`mongosh`)**:
```javascript
// Query photos captured in October 2026 with an aperture of f/2.8 or wider
db.items.find({
  "temporal.year": 2026,
  "temporal.month": 10,
  "exposure.f_number": { $lte: 2.8 }
});

// Find exact duplicate candidates using BLAKE3 cryptographic hash
db.items.aggregate([
  { $group: { _id: "$fingerprint.cryptographic_hash", count: { $sum: 1 }, files: { $push: "$file_context.file_path" } } },
  { $filter: { count: { $gt: 1 } } }
]);
```

---

## 4. Automated Verification Commands

```bash
# Run complete test suite
cargo test

# Verify formatting and linter
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```
