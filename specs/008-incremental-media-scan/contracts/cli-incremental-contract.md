# Interface Contract: CLI Flags, Pre-Filtering & Summary Reporting

**Feature**: `008-incremental-media-scan`  
**Contract Type**: CLI & Terminal Telemetry Contract  
**Status**: Approved  

---

## 1. CLI Arguments & Environment Flags

| Argument | Long Flag | Environment Variable | Type | Default | Description |
|---|---|---|---|---|---|
| | `--force` / `--reindex` | `MEDIA_FORCE_REINDEX` | Boolean flag | `false` | Force complete re-extraction of all files, bypassing database incremental change pre-filtering |

---

## 2. Interactive Terminal Progress Output Contract

When incremental skipping occurs during an interactive scan:

```text
[Extraction] ⠋ [===================>       ] 80% | 13,420/13,420 items (Skipped: 13,400 | New: 20) | 1,450 items/s | Elapsed: 2s | ETA: 0s
```

### Components
1. `Skipped: <count>`: Number of files verified as already indexed and unmodified.
2. `New / Modified: <count>`: Number of files undergoing full metadata extraction.
3. `Rate`: Reflects high effective throughput (often thousands of items/second when skipping).

---

## 3. Final Execution Summary Report Contract

```text
================================================================================
Metadata Extraction Execution Summary
--------------------------------------------------------------------------------
Session ID:          d9b407fe-d0ec-40f4-a0e4-b78807d9b9d3
Target Path:         /spsa/temp
Performance Mode:    Fast (Incremental change caching active)
Total Duration:      2.14s
Items Discovered:    13,420
Items Skipped:       13,400 (Already indexed & unmodified)
Items Extracted:     20 (New: 15 | Modified: 5)
Data Volume:         1.02 TB (Processed: 1.2 GB | Cached: 1.018 TB)
Average Throughput:  6,271 items/sec (Effective)
Database Ingestion:  15 upserted | 5 modified | 13,400 unchanged
Status:              Completed successfully (0 errors, 0 warnings)
================================================================================
```
