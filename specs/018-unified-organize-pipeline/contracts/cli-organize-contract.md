# CLI Contract: Organize Subcommand

## Command Invocation

```bash
# Full end-to-end organize pipeline
cargo run -- organize --source /Volumes/SP-1/SPSA/unorganized --config drives.yaml

# Full dry-run preview across all 5 stages
cargo run -- organize --source /Volumes/SP-1/SPSA/unorganized --dry-run

# Organize and replicate without pruning source
cargo run -- organize --source /Volumes/SP-1/SPSA/unorganized --skip-prune
```

## Terminal Output Format

```text
================================================================================
MEDIA ORGANIZATION PIPELINE: STAGE EXECUTION
================================================================================
>>> Stage [1/5]: Extracting Metadata from Source Directory...
[Extraction] [============================] 100% | 150/150 items | Elapsed: 2.1s

>>> Stage [2/5]: Auditing Catalog Duplicates & Replicas...
[Audit] Canonical Masters: 1645 | Replicas: 1645 | Pending Ingest: 150 | Elapsed: 0.4s

>>> Stage [3/5]: Ingesting Unique Files into Canonical Primary Master...
[Ingestion] [============================] 100% | 150/150 files (1.20 GB) | Elapsed: 5.3s

>>> Stage [4/5]: Synchronizing Replicas to Backup Drives...
[Replication] [==========================] 100% | 150/150 files (1.20 GB) | Elapsed: 5.8s

>>> Stage [5/5]: Pruning Redundant Files & Cleaning Empty Directories...
[Prune] Files Deleted: 150 | Empty Directories Removed: 8 | Reclaimed: 1.20 GB | Elapsed: 0.6s

================================================================================
MEDIA ORGANIZATION PIPELINE: FINAL SUMMARY
================================================================================
Source Directory:          /Volumes/SP-1/SPSA/unorganized
Configuration:             drives.yaml
Mode:                      Execution (Live Disk Updates)
Files Extracted:           150
Canonical Masters Ingested:150 (1.20 GB)
Replicas Synchronized:     150 (1.20 GB across replica_1)
Redundant Files Pruned:    150
Empty Directories Removed: 8
Storage Space Reclaimed:   1.20 GB
Total Pipeline Time:       14.2s
Status:                    SUCCESS
================================================================================
```
