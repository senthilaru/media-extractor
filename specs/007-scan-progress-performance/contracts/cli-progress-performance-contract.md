# Interface Contract: CLI Flags, Telemetry & Summary Reporting

**Feature**: `007-scan-progress-performance`  
**Contract Type**: CLI & Terminal Telemetry Specification  
**Status**: Approved  

---

## 1. CLI Arguments & Environment Flags

The CLI interface for `metadata-extractor` (root command and `extract` subcommand) is extended with performance and progress controls:

| Argument | Long Flag | Environment Variable | Type / Values | Default | Description |
|---|---|---|---|---|---|
| `-f` | `--fast` | `MEDIA_FAST_MODE` | Boolean flag | `false` | Enable high-speed metadata-only extraction; bypass full-file bitwise and perceptual hashing |
| | `--mode` | `MEDIA_EXTRACTION_MODE` | `fast`, `balanced`, `full` | `balanced` (or `fast` if `--fast` passed) | Set explicit extraction performance mode |
| | `--max-hash-size` | `MEDIA_MAX_HASH_SIZE_MB` | Unsigned Integer (MB) | `100` | Skip full bitwise hashing for media files exceeding this size threshold (MB) |
| | `--workers` | `MEDIA_WORKERS` | Unsigned Integer | Host logical CPU cores | Number of concurrent batch extraction workers |
| `-q` | `--quiet` | `MEDIA_QUIET` | Boolean flag | `false` | Suppress all live progress indicators on stderr |
| | `--json-telemetry` | `MEDIA_JSON_TELEMETRY` | Boolean flag | `false` | Emit progress telemetry as discrete JSON lines instead of ANSI progress bar |

---

## 2. Interactive Terminal Progress Output Contract (TTY)

When standard error is connected to a TTY (`std::io::stderr().is_terminal() == true`) and `--quiet` is false:

### Format Specification
```text
[Phase] ⠋ [===================>       ] 75% | 13,420/17,800 items (1.02/1.35 TB) | 62.4 items/s | 145.2 MB/s | Elapsed: 3m 42s | ETA: 1m 10s
```

### Components
1. `[Phase]`: Active phase (`[Discovery]`, `[Extraction]`, `[Fingerprinting]`, `[Ingesting]`, `[Flushing]`).
2. `⠋`: Steady-tick animated spinner indicating continuous non-blocking execution.
3. `[=====>  ] 75%`: Rendered progress bar. If total items is still discovering, renders a pulsing indeterminate bar (`[<=>     ]`).
4. `Items & Volume`: Formatted item count and human-readable byte volume (`B`, `KB`, `MB`, `GB`, `TB`).
5. `Rates`: Instantaneous / moving-average item throughput (`items/s`) and byte throughput (`MB/s`).
6. `Timing`: Elapsed time (`Xm Ys`) and moving-average estimated remaining time (`ETA: Xm Ys`).

---

## 3. Non-Interactive Output Contract (Pipes / CI / Redirects)

When standard error is redirected to a file or pipe (`is_terminal() == false`), the ANSI cursor commands are omitted. The system emits discrete single-line updates at a 5-second cadence:

```text
2026-11-08T23:45:00Z INFO [Progress] Phase: Extraction | Processed: 5,000/15,000 (33.3%) | Rate: 58.2 items/s (120.4 MB/s) | Elapsed: 1m 25s | ETA: 2m 50s | Errors: 0
```

If `--json-telemetry` is enabled:
```json
{"type":"progress","session_id":"c6b907fe-d0ec-40f4-a0e4-b78807d9b9d3","phase":"Extraction","items_processed":5000,"items_total":15000,"bytes_processed":350000000000,"throughput_fps":58.2,"throughput_mbps":120.4,"elapsed_secs":85.0,"eta_secs":170.0,"errors":0}
```

---

## 4. Final Execution Summary Report Contract

Upon completion or Ctrl+C cancellation, the system prints the final summary on `stderr`:

```text
================================================================================
Metadata Extraction Execution Summary
--------------------------------------------------------------------------------
Session ID:          c6b907fe-d0ec-40f4-a0e4-b78807d9b9d3
Target Path:         /spsa/temp
Performance Mode:    Fast (Metadata-only; full-file hashing skipped)
Total Duration:      3m 42s (222.45s)
Items Discovered:    13,420
Items Extracted:     13,420 (100.0%)
Data Volume:         1.02 TB (1,024,850,210,400 bytes)
Average Throughput:  60.3 items/sec (4.6 GB/min)
Database Ingestion:  13,420 upserted | 0 modified | 0 failed
Status:              Completed successfully (0 errors, 2 warnings)
================================================================================
```

---

## 5. Exit Code Contract

| Exit Code | Condition | Description |
|---|---|---|
| `0` | Success | All files traversed and extracted successfully (warnings permitted) |
| `1` | General Failure | Unrecoverable I/O error or invalid root path |
| `2` | Configuration Error | Invalid profile, missing production credentials, or invalid CLI flag |
| `3` | Storage Error | MongoDB connection/index failure |
| `130` | Interrupted | User cancelled with SIGINT (Ctrl+C); partial summary emitted |
