# Quickstart: Ingestion of Unmastered Files to Primary Master

This guide walks through validating Feature `010-ingest-to-master` end-to-end.

---

## 1. Prerequisites

1. MongoDB instance running locally or via connection string:
   ```bash
   export MONGODB_URI="mongodb://localhost:27017/media_index_db"
   ```
2. Drive configuration file `drives.yaml` defining a primary drive and a secondary/dump drive:
   ```yaml
   version: "1.0"
   groups:
     primary:
       priority: 100
       description: "Primary Master Storage"
     scratch:
       priority: 40
       description: "Temporary Camera Dumps"

   drives:
     - id: "primary_main"
       group: "primary"
       mount_path: "/Volumes/SP-1"
       master_path: "/Volumes/SP-1/SPSA/media/master"

     - id: "seagate_dump"
       group: "scratch"
       mount_path: "/Volumes/Dump-1"
       master_path: "/Volumes/Dump-1/master"
   ```

---

## 2. Validation Scenarios

### Scenario A: Dry-Run Preview
Preview what files would be copied and how they will be structured in the master directory without modifying any files or database state:

```bash
metadata-extractor ingest --source-drive seagate_dump --config drives.yaml --dry-run
```

**Expected Outcome**:
- Terminal displays the planned transfers showing source path, computed `<Year>/<DD-Mon-YYYY>/<CategoryFolder>/<filename>` destination path, and file size.
- Zero bytes are written to disk.
- Zero MongoDB records are changed.

---

### Scenario B: Execute Ingestion with Deferred Cleanup
Copy files into the primary master, verify BLAKE3 integrity, register canonical masters in MongoDB, and mark source files for deletion:

```bash
metadata-extractor ingest --source-drive seagate_dump --config drives.yaml
```

**Expected Outcome**:
- Files appear in `/Volumes/SP-1/SPSA/media/master/<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}/`.
- New MongoDB records exist with `dedup.status = "CanonicalMaster"`.
- Original records on `seagate_dump` transition from `NeedsIngestToMaster` to `MarkForRemoval`.
- Original files remain intact on `seagate_dump` for subsequent pruning.

---

### Scenario C: Verify with `dedup prune`
Clean up the marked duplicate files on the secondary drive:

```bash
metadata-extractor dedup prune --drive seagate_dump --config drives.yaml
```

**Expected Outcome**:
- Shows files ready to delete, confirming the canonical master exists in `/Volumes/SP-1`.
- When re-run with `--execute`, redundant files on `seagate_dump` are deleted and their MongoDB status is marked as `Removed`.

---

### Scenario D: One-Step Ingest and Source Deletion (`--delete-source`)
Transfer and immediately remove source files in a single command:

```bash
metadata-extractor ingest --source-drive seagate_dump --config drives.yaml --delete-source
```

**Expected Outcome**:
- Verified files are copied to the primary master.
- Verified source files are removed from the secondary drive.
- MongoDB records are updated to `Removed`.
