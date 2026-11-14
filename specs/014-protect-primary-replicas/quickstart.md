# Quickstart Validation Guide: Archive Immutability & Deletion Protection

This guide outlines how to verify that primary master and replica storage paths are completely protected from file deletion.

## Scenario 1: Attempting to Prune Primary Master Drive

```bash
cargo run -- dedup prune --drive primary_main --config drives.yaml
```

**Expected Result**:
- Fails immediately with `PruneForbiddenOnProtectedArchive`.
- Zero database records modified.
- Zero files inspected or removed.

---

## Scenario 2: Attempting to Prune Replica Drive

```bash
cargo run -- dedup prune --drive replica1 --config drives.yaml
```

**Expected Result**:
- Fails immediately with `PruneForbiddenOnProtectedArchive`.
- Zero database records modified.
- Zero files inspected or removed.

---

## Scenario 3: Attempting to Prune Primary Master via Path

```bash
cargo run -- dedup prune --drive /Volumes/SP-1/SPSA/media/master --config drives.yaml
```

**Expected Result**:
- System detects path resolves inside Primary Master hierarchy.
- Fails immediately with `PruneForbiddenOnProtectedArchive`.

---

## Scenario 4: Ingestion with `--delete-source` on Protected Archive

```bash
cargo run -- ingest --source-drive /Volumes/SP-1/SPSA/media/master --delete-source --config drives.yaml
```

**Expected Result**:
- System refuses to delete any file from the protected master root.
- All files remain intact.
