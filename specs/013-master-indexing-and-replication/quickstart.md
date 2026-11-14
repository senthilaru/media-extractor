# Quickstart Validation Guide: Master Indexing and Replication Lifecycle

This guide describes how to validate the end-to-end 3-step archival workflow on a folder containing 5 copies of the same media file under different filenames.

## Prerequisites

1. MongoDB running locally at `mongodb://localhost:27017` (database `media_index_db`).
2. A sample source folder with 5 copies of a video/photo (e.g. `/tmp/spsa_test/samples` containing `copy1.mp4`, `copy2.mp4`, `copy3.mp4`, `copy4.mp4`, `copy5.mp4`).
3. A primary master target directory (e.g. `/tmp/spsa_test/master`).
4. A drives configuration file (e.g. `drives.yaml`).

---

## Step 1: Discover & Index Assets

Scan and extract metadata for all files in the test directory:

```bash
cargo run -- /tmp/spsa_test/samples
```

**Expected Outcome**:
- 5 documents inserted into MongoDB under collection `media_assets`.
- Each document has `file_context.file_path` pointing to its respective file in `/tmp/spsa_test/samples`.
- Each document has identical `fingerprint.cryptographic_hash`.

---

## Step 2: Run Deduplication Evaluation

Group identical assets and elect the single best copy for primary ingestion:

```bash
cargo run -- dedup --all
```

**Expected Outcome**:
- 1 duplicate cluster detected containing 5 files.
- Exactly 1 file designated with `dedup.status: NeedsIngestToMaster`.
- Exactly 4 files designated with `dedup.status: MarkForRemoval` (with `canonical_master_id: null` initially).

---

## Step 3: Ingest Into Master Archive

Ingest the elected copy into the primary master volume:

```bash
cargo run -- ingest --source-drive /tmp/spsa_test/samples --config drives.yaml
```

**Expected Outcome**:
- Exactly 1 file is transferred into the primary master structured directory (`<Year>/<DD-Mon-YYYY>/video/...`).
- A new document is added to MongoDB:
  - `file_context.file_path` = path on the primary master volume.
  - `dedup.status` = `CanonicalMaster`.
  - `dedup.canonical_master_id` = master record's UUID.
  - `dedup.replication.status` = `Pending` (or `Replicated` if 0 replica drives configured).
- All 5 source records in `/tmp/spsa_test/samples` now have:
  - `dedup.status` = `MarkForRemoval`.
  - `dedup.canonical_master_id` = master record's UUID.
  - `dedup.canonical_master_path` = path on the primary master volume.

---

## Step 4: Verify Pruning Integrity

Run duplicate pruning on the source directory:

```bash
cargo run -- dedup prune --drive /tmp/spsa_test/samples --execute
```

**Expected Outcome**:
- All 5 copies on `/tmp/spsa_test/samples` are verified against the canonical master on the primary drive.
- All 5 copies are deleted from `/tmp/spsa_test/samples`.
- Zero "unverified master" skips or errors.
- Master file remains intact and verified on the primary master drive.
