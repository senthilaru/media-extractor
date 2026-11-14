# Quickstart & Validation Guide: Database Content & Health Audit Report

**Feature**: `020-database-content-report`  
**Date**: 2026-11-14

## Overview

This guide details how to validate the database audit report and sample inspection features against a local or containerized MongoDB database.

---

## Prerequisites

1. MongoDB running locally or accessible via URI (e.g. `mongodb://localhost:27017`).
2. Configured environment:
   ```bash
   export MEDIA_MONGODB_URI="mongodb://localhost:27017"
   export MEDIA_MONGODB_DATABASE="media_metadata"
   ```

---

## Validation Scenarios

### Scenario 1: Generate Standard Terminal Report

Execute the primary reporting command:

```bash
cargo run -- report
```

**Expected Outcome**:
- Clean terminal output rendering:
  - Total cataloged files count.
  - Grouped breakdown of file extensions (`.jpg`, `.heic`, `.mp4`, etc.) with percentages.
  - Category tallies for copied to master, yet to replicate, duplicates, marked for deletion, missing EXIF, and problem files.
  - Prominent "SUGGESTED NEXT ACTION" section recommending the next command (or confirming all healthy).
  - Exit code `0`.

---

### Scenario 2: Inspect Problematic Files (Top 3 Samples)

Execute the inspection subcommand to view problematic files:

```bash
cargo run -- report inspect problems --limit 3
```

**Expected Outcome**:
- Terminal lists up to 3 filepaths identified as having extraction errors.
- Displays the error message/diagnostic for each file.
- Does not fail if fewer than 3 items exist (displays all available).

---

### Scenario 3: Inspect Files Missing EXIF Metadata

```bash
cargo run -- report inspect missing-exif
```

**Expected Outcome**:
- Lists up to 3 filepaths where `temporal.source` fell back to filesystem timestamps.
- Displays file size and last modified date.

---

### Scenario 4: Export Machine-Readable JSON

```bash
cargo run -- report --format json | jq .
```

**Expected Outcome**:
- Emits valid JSON complying with [`report-json-schema.json`](file:///Users/spsenthil/Documents/repo/media/media-extractor/specs/020-database-content-report/contracts/report-json-schema.json).
- Exit code `0`.

---

### Scenario 5: Contextual Action Suggestion Verification

1. When unmastered files exist in database:
   - Output suggests `cargo run -- ingest`
2. When pending replicas exist in database:
   - Output suggests `cargo run -- sync-replicas`
3. When duplicates awaiting prune exist:
   - Output suggests `cargo run -- organize`
4. When all assets are ingested, replicated, and clean:
   - Output indicates system is fully synchronized and healthy.
