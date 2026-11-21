# Quickstart: Multi-Volume Storage Extension Pools

## Verification Scenario 1: Configure Multi-Volume Storage Pools

1. In `drives.yaml`, define primary and replica pools with extension volumes:
   ```yaml
   groups:
     primary:
       priority: 100
     replica1:
       priority: 80

   drives:
     - id: "primary_vol1"
       group: "primary"
       mount_path: "/Volumes/SP-1"
       master_path: "/Volumes/SP-1/SPSA/media/master"

     - id: "primary_vol2"
       group: "primary"
       mount_path: "/Volumes/SP-3"
       master_path: "/Volumes/SP-3/SPSA/media/master"

     - id: "replica_1_vol1"
       group: "replica1"
       mount_path: "/Volumes/SP-2"
       master_path: "/Volumes/SP-2/SPSA/media/master"

     - id: "replica_1_vol2"
       group: "replica1"
       mount_path: "/Volumes/SP-4"
       master_path: "/Volumes/SP-4/SPSA/media/master"
   ```

## Verification Scenario 2: Deduplication Enforces Single Copy Across Volumes

1. Place duplicate copies of a photo in `primary_vol1/2026/01-Jan-2026/Photos/` and `primary_vol2/2026/01-Jan-2026/Photos/`.
2. Run duplicate audit:
   ```bash
   cargo run -- dedup -c drives.yaml
   ```
3. Verify that only one copy is marked `CanonicalMaster`, and the other is marked `MarkForRemoval`.
4. Pruning removes the redundant intra-pool copy:
   ```bash
   cargo run -- dedup prune -c drives.yaml --execute
   ```

## Verification Scenario 3: Automated Ingest Spillover

1. When `primary_vol1` is full, ingest new unmastered files:
   ```bash
   cargo run -- ingest -c drives.yaml
   ```
2. Ingestion automatically writes to `primary_vol2` under the standard date structure without failing.

## Verification Scenario 4: Automated Test Suite

Run unit and integration tests verifying multi-volume pools, single-copy invariants, spillover, and replica mirroring:
```bash
cargo test --test test_storage_pools
```
