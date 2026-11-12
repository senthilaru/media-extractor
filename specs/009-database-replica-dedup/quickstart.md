# Quickstart Guide: Database-Driven Replica Deduplication & Drive Pruning

**Feature**: `009-database-replica-dedup`  
**Status**: Ready for Validation  

---

## Scenario: Consolidating and Cleaning 10 External Hard Drives

### Step 1: Prepare Drive Configuration (`drives.yaml`)

Create `drives.yaml` in your project root or working directory:

```yaml
version: "1.0"
groups:
  primary:
    priority: 100
    description: "Primary home media server master storage"
  replica1:
    priority: 80
    description: "First complete mirror backup"

drives:
  - id: "primary_disk"
    group: "primary"
    mount_path: "/Volumes/SP-1"
    master_path: "/Volumes/SP-1/SPSA/media/master"

  - id: "replica1_disk"
    group: "replica1"
    mount_path: "/Volumes/SP-2"
    master_path: "/Volumes/SP-2/SPSA/media/master"

  - id: "scratch_disk"
    group: "replica1"
    mount_path: "/Volumes/SP-3"
    master_path: "/Volumes/SP-3/SPSA/media/master"
```

---

### Step 2: Index External Hard Drives One by One into MongoDB

*(If already done, skip directly to Step 3)*

```bash
# Connect Drive 1:
metadata-extractor /Volumes/SP-1

# Disconnect Drive 1, Connect Drive 2:
metadata-extractor /Volumes/SP-2

# Disconnect Drive 2, Connect Drive 3:
metadata-extractor /Volumes/SP-3
```

---

### Step 3: Run Database-Driven Deduplication to Mark Duplicates

Deduplication executes **directly against the MongoDB catalog** with no requirement that all 10 hard drives are mounted at the same time:

```bash
# 1. Preview deduplication analysis without updating the database (Dry-Run):
metadata-extractor dedup --config drives.yaml --dry-run

# 2. Execute deduplication and mark duplicate statuses in MongoDB:
metadata-extractor dedup --config drives.yaml
```

---

### Step 4: Plug in One Drive and Prune Marked Duplicates Safely

Connect the drive you want to clean up (for example, `/Volumes/SP-3`):

```bash
# 1. Preview files that will be deleted from Drive 3 (Safe Simulation):
metadata-extractor dedup prune --drive /Volumes/SP-3

# 2. Save an audit manifest of removal candidates:
metadata-extractor dedup prune --drive /Volumes/SP-3 --manifest-out ./drive3_manifest.json

# 3. Physically prune redundant duplicate files once verified:
metadata-extractor dedup prune --drive /Volumes/SP-3 --execute
```
