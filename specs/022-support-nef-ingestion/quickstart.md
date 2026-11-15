# Quickstart: Validating Nikon NEF Raw Ingestion

**Feature**: Ingest Nikon NEF Raw Format  
**Branch**: `022-support-nef-ingestion`  
**Date**: 2026-11-15  

## Prerequisites
- MongoDB running on `localhost:27017` with valid credentials in `.env`.
- Master and replica drives mounted (`/Volumes/SP-1`, `/Volumes/SP-2`).

## Validation Scenarios

### Scenario 1: Verify Full-Resolution NEF Dimension Extraction
Extract metadata from a real Nikon NEF file:
```bash
cargo run -- extract "/Volumes/Seagate Backup Plus Drive/SPSA/Halloween/SPS_1127.NEF"
```
**Expected Outcome**:
- Output JSON displays:
  - `media_category`: `"RawPhoto"`
  - `dimensions.width`: `6036` (or `6000`)
  - `dimensions.height`: `4020` (or `4000`)
  - `fingerprint.cryptographic_hash`: 64-character BLAKE3 string.

### Scenario 2: Verify Self-Healing Incremental Scan
Run an incremental scan on a directory containing previously failed NEF entries:
```bash
cargo run -- scan "/Volumes/Seagate Backup Plus Drive/SPSA/Halloween"
```
**Expected Outcome**:
- Incremental pre-filtering detects failed or un-fingerprinted records and re-extracts them.
- All 103 NEF files report `status: "Success"`.

### Scenario 3: Verify Master Ingestion and Replica Mirroring
Run organize dry-run or live ingestion:
```bash
cargo run -- organize --source "/Volumes/Seagate Backup Plus Drive/SPSA/Halloween" -c drives.yaml --dry-run
```
**Expected Outcome**:
- Ingestion planner creates copy tasks to:
  `/Volumes/SP-1/SPSA/media/master/2019/25-Oct-2019/PhotosRaw/SPS_1127.NEF`
- Replication planner mirrors to:
  `/Volumes/SP-2/SPSA/media/master/2019/25-Oct-2019/PhotosRaw/SPS_1127.NEF`
