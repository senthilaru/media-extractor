# CLI Interface Contract: Ingestion & Replication Telemetry

## 1. Master Ingestion Command Output

When executing `cargo run -- ingest --source-drive <SOURCE> --config drives.yaml`, the summary output is structured as follows:

```text
================================================================================
MASTER INGESTION SUMMARY
================================================================================
Source Drive:          samples (/Users/spsenthil/Documents/spsa/samples)
Target Primary Master: primary_main (/Volumes/SP-1/SPSA/media/master)
Status:                SUCCESS (100% Verified)
Files Transferred:     1
Files Skipped:         0 (Identical already in master)
Files Failed:          0
Volume Ingested:       223382924 bytes (213.03 MB)
Elapsed Time:          0.450s
--------------------------------------------------------------------------------
Database Updates:
  - Canonical Masters Created: 1 (Registered on primary_main)
  - Duplicate Records Linked:  4 (Updated with canonical master ID & path)
  - Source Files Marked:       5 (Set to 'MarkForRemoval' on samples)
Replication Status:
  - Pending Replication:       1 assets awaiting sync to [replica1, replica2]
Source Files on Disk:  Preserved on source drive. Run `dedup prune --drive samples` to delete.
================================================================================
```

---

## 2. Replication Synchronization Command Output

When executing `cargo run -- sync-replicas --config drives.yaml` (or `replicate`):

```text
================================================================================
REPLICA SYNCHRONIZATION SUMMARY
================================================================================
Primary Master Drive:  primary_main (/Volumes/SP-1/SPSA/media/master)
Configured Replicas:   replica1, replica2
Status:                SUCCESS (100% Verified)
Files Replicated:      2 (1 per replica target)
Files Skipped:         0
Files Failed:          0
Bytes Transferred:     446765848 bytes (426.06 MB)
Elapsed Time:          0.820s
--------------------------------------------------------------------------------
Database Updates:
  - Replica Records Created:   2 ('ReplicaKeep')
  - Master Records Updated:    1 (Transitioned to 'Replicated')
================================================================================
```
