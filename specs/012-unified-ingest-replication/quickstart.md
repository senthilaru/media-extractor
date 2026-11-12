# Quickstart Validation Guide: Unified Ingestion & Multi-Replica Synchronization

**Feature Branch**: `012-unified-ingest-replication`  
**Date**: 2026-11-10  
**Context**: End-to-end validation scenarios for verifying universal asset classification, master ingestion, and multi-replica fanout synchronization.

---

## 1. Prerequisites & Environment Setup

Ensure the Rust toolchain and MongoDB local instance (or mock test environment) are available:

```bash
# Verify rust toolchain
cargo --version

# Copy sample drive configuration for validation if not already present
cp drives.example.yaml drives.yaml
```

Set up temporary test mock drives (or export test environment variables):

```bash
export TEST_DIR=$(mktemp -d)
export DRIVE_PRIMARY_MOUNT="${TEST_DIR}/primary"
export DRIVE_PRIMARY_MASTER="${TEST_DIR}/primary/SPSA/media/master"
export DRIVE_REPLICA1_MOUNT="${TEST_DIR}/replica1"
export DRIVE_REPLICA1_MASTER="${TEST_DIR}/replica1/SPSA/media/master"
export DRIVE_REPLICA2_MOUNT="${TEST_DIR}/replica2"
export DRIVE_REPLICA2_MASTER="${TEST_DIR}/replica2/SPSA/media/master"
export DRIVE_STAGING_MOUNT="${TEST_DIR}/staging"

mkdir -p "${DRIVE_PRIMARY_MASTER}" "${DRIVE_REPLICA1_MASTER}" "${DRIVE_REPLICA2_MASTER}" "${DRIVE_STAGING_MOUNT}/import"
```

---

## 2. Validation Scenario 1: Universal Classification of Unique & Duplicate Assets

Proves that both unique assets and duplicate clusters residing on staging drives are properly classified.

### Setup
Generate test media on the staging drive:
- 3 unique photos (`unique1.jpg`, `unique2.jpg`, `unique3.jpg`)
- 2 duplicate photos (`dup_a.jpg`, `dup_b.jpg` with identical bytes)

```bash
echo "unique image 1 payload" > "${DRIVE_STAGING_MOUNT}/import/unique1.jpg"
echo "unique image 2 payload" > "${DRIVE_STAGING_MOUNT}/import/unique2.jpg"
echo "unique image 3 payload" > "${DRIVE_STAGING_MOUNT}/import/unique3.jpg"
echo "duplicate image identical payload" > "${DRIVE_STAGING_MOUNT}/import/dup_a.jpg"
echo "duplicate image identical payload" > "${DRIVE_STAGING_MOUNT}/import/dup_b.jpg"
```

### Execution
Scan and run universal catalog classification:

```bash
# 1. Scan and index staging drive
cargo run -- scan "${DRIVE_STAGING_MOUNT}" --mongodb-uri "${MONGODB_URI:-mongodb://localhost:27017/metadata_test}"

# 2. Run universal classification preview
cargo run -- dedup --all --dry-run --config drives.yaml
```

### Expected Outcome
- All 3 unique photos are classified as `NeedsIngestToMaster`.
- Exactly 1 copy of the duplicate pair (`dup_a.jpg`) is classified as `NeedsIngestToMaster`.
- The redundant duplicate copy (`dup_b.jpg`) is classified as `MarkForRemoval`.
- Total assets queued for ingestion: exactly 4.

---

## 3. Validation Scenario 2: Master Ingestion with Bitwise Verification

Proves that unmastered assets are transferred to the primary master drive in canonical date/category structure with bitwise hash verification.

### Execution
Run master ingestion:

```bash
cargo run -- ingest --source-drive staging_drive --config drives.yaml
```

### Expected Outcome
- 4 files are copied into `${DRIVE_PRIMARY_MASTER}/<Year>/<DD-Mon-YYYY>/Photos/`.
- Every transferred file has its BLAKE3 hash verified against the source before catalog update.
- Destination files are registered in MongoDB as `CanonicalMaster`.
- Source staging files are transitioned to `MarkForRemoval` with `canonical_master_id` referencing the new master.

---

## 4. Validation Scenario 3: Multi-Replica Synchronization & Fanout

Proves that verified canonical master assets are synchronized to all connected replica drives in an identical hierarchy.

### Execution
Run replica synchronization:

```bash
# Preview replication
cargo run -- sync-replicas --dry-run --config drives.yaml

# Execute replication fanout
cargo run -- sync-replicas --config drives.yaml
```

### Expected Outcome
- Both mounted replica drives (`replica1` and `replica2`) receive exact copies of all 4 canonical master files under their respective `<Year>/<DD-Mon-YYYY>/Photos/` folders.
- Each replica copy is bitwise hash verified.
- MongoDB catalog records 4 new `ReplicaKeep` records for `replica1` and 4 new `ReplicaKeep` records for `replica2`.
- If `replica2` was unmounted (`rmdir` or detached), synchronization succeeds for `replica1` and reports `replica2` as offline without error.

---

## 5. Validation Scenario 4: Combined Ingest & Fanout (`--replicate`)

Proves the single-step unified command workflow.

### Setup
Add a new unmastered asset:
```bash
echo "newly taken photo 2026" > "${DRIVE_STAGING_MOUNT}/import/new_photo.jpg"
cargo run -- scan "${DRIVE_STAGING_MOUNT}" --mongodb-uri "${MONGODB_URI:-mongodb://localhost:27017/metadata_test}"
cargo run -- dedup --all --config drives.yaml
```

### Execution
```bash
cargo run -- ingest --source-drive staging_drive --replicate --config drives.yaml
```

### Expected Outcome
1. `new_photo.jpg` is ingested into `${DRIVE_PRIMARY_MASTER}/<Year>/<DD-Mon-YYYY>/Photos/new_photo.jpg` and registered as `CanonicalMaster`.
2. Master ingestion immediately triggers replica fanout.
3. Both `replica1` and `replica2` receive verified copies in their master hierarchies as `ReplicaKeep`.
4. Staging source file transitions to `MarkForRemoval`.
5. Progress bars and elapsed duration summary are visibly printed for both ingest and replication stages.

---

## 6. Automated Test Suite Execution

Run the complete test suite verifying these contracts:

```bash
# Unit and integration tests
cargo test --test test_master_ingest
cargo test --test test_database_dedup
cargo test --test unit
```
