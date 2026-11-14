# CLI Interface Contract: Archive Deletion Protection

## 1. Forbidden Pruning on Primary Master Volume

When an operator executes `dedup prune` targeting a primary master drive (by ID or path):

```bash
cargo run -- dedup prune --drive primary --config drives.yaml
```

**Console Output (stderr)**:
```text
Error: Pruning is strictly forbidden on protected archive storage 'primary' (/Volumes/SP-1/SPSA/media/master): Primary master volumes are permanent, immutable archives. Deletion operations are completely blocked.
```
**Exit Code**: Non-zero (e.g., 1).

---

## 2. Forbidden Pruning on Replica Volume

When an operator executes `dedup prune` targeting a replica backup drive:

```bash
cargo run -- dedup prune --drive replica1 --config drives.yaml
```

**Console Output (stderr)**:
```text
Error: Pruning is strictly forbidden on protected archive storage 'replica1' (/Volumes/Backup/SPSA/media/master): Replica backup volumes are permanent mirrors of the master archive. Deletion operations are completely blocked.
```
**Exit Code**: Non-zero.

---

## 3. Ingestion Source Deletion Warning

When an operator executes `ingest --delete-source` on a source path that resides within primary or replica storage:

```bash
cargo run -- ingest --source-drive /Volumes/SP-1/SPSA/media/master/staging --delete-source --config drives.yaml
```

**Console Output**:
```text
WARNING: Source path '/Volumes/SP-1/SPSA/media/master/staging' is located on protected archive storage. Source deletion (--delete-source) was blocked to prevent data loss. 0 files deleted.
```
