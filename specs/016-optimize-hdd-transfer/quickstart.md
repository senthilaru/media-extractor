# Quickstart & Validation Guide: External HDD Transfer Optimization

**Feature**: `016-optimize-hdd-transfer`
**Date**: 2026-11-13

## 1. Prerequisites
- Rust 1.75+ toolchain installed.
- Local MongoDB running at `localhost:27017`.
- Source and Destination storage directories configured in `drives.yaml`.

---

## 2. Validation Scenarios

### Scenario 1: Verify Default 8 MB Buffer & In-Flight Hashing on Ingest
```bash
# Execute ingest from an unorganized directory into master
cargo run -- ingest \
  --source-drive /Volumes/SP-1/SPSA/unorganized/Photos/Feb2026 \
  --config drives.yaml
```

**Expected Outcome**:
- Pre-ingest overview prints total unmastered files and payload size.
- Real-time progress bar reflects contiguous 8 MB chunk streaming with sustained throughput (>120 MB/s on USB 3.0 HDDs).
- Verification completes immediately upon last block write without a secondary destination disk read pass.
- Ingest execution report outputs `Status: SUCCESS (100% Verified)`.

---

### Scenario 2: Test Custom Buffer Sizing Parameter
```bash
# Run with explicit 16 MB buffer chunk
cargo run -- ingest \
  --source-drive /Volumes/SP-1/SPSA/unorganized/Photos/Feb2026 \
  --config drives.yaml \
  --buffer-size 16M \
  --dry-run
```

**Expected Outcome**:
- Plan compiles cleanly.
- When executed, allocates 16 MB bounded streaming buffers without memory growth.

---

### Scenario 3: Verify Optimized Replica Synchronization
```bash
# Sync canonical masters from primary to replica
cargo run -- sync-replicas -c drives.yaml
```

**Expected Outcome**:
- Replica sync shows real-time progress bar with live transfer rate.
- Transfers execute sequentially on `/Volumes/SP-2` without head-seeking degradation.
- Zero hash mismatch errors occur due to synchronous `sync_all` cache flush.

---

### Scenario 4: Automated Test Suite Validation
```bash
cargo test --test test_master_ingest
cargo test --test test_replica_sync
cargo test --test test_progress_performance_integration
```

**Expected Outcome**:
- All unit and integration tests pass with 0 failures.
