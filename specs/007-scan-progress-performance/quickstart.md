# Quickstart & Validation Guide: Scan Progress & High-Throughput Performance

**Feature**: `007-scan-progress-performance`  
**Date**: 2026-11-08  
**Status**: Ready for Validation  

---

## 1. Overview

This guide details executable validation scenarios to verify real-time terminal progress indicators, moving-average ETA calculations, high-throughput fast-mode scanning, large-file size thresholds, and post-scan execution summary reporting.

---

## 2. Prerequisites & Build

Build the release binary:
```bash
cargo build --release
```
The binary will be located at `./target/release/metadata-extractor`.

---

## 3. Validation Scenarios

### Scenario 1: Interactive High-Speed Scan with Live Progress & ETA
Test fast-mode scanning on a media directory to verify the live interactive progress bar, throughput rates, and final execution summary:

```bash
# Run in fast mode on the media directory
./target/release/metadata-extractor /spsa/temp --fast
```

**Expected Outcome**:
- `stderr` displays real-time progress updating at ~4 fps:
  ```text
  [Extraction] ⠋ [===============>         ] 64% | 8,500/13,420 items (650 GB) | 65.2 items/s | 180 MB/s | Elapsed: 2m 10s | ETA: 1m 15s
  ```
- Instantaneous throughput reflects high speed (>50 files/sec on SSD).
- Upon scan completion, a complete boxed `Execution Summary` is emitted on `stderr`.
- Process exits with code `0`.

---

### Scenario 2: Standard Output Redirection & Clean Piped Streams
Verify that progress bars and telemetry write strictly to `stderr`, leaving `stdout` 100% clean for JSON stream consumption:

```bash
# Redirect stdout to a JSON lines file and check validity
./target/release/metadata-extractor /spsa/temp --fast > /tmp/extracted_output.jsonl

# Validate that every line in the output file is valid JSON
jq empty /tmp/extracted_output.jsonl
echo "JSON validation exit code: $?"
```

**Expected Outcome**:
- Progress bars and status indicators appear on the terminal (`stderr`).
- `/tmp/extracted_output.jsonl` contains purely valid JSON records with zero ANSI escape codes or progress text.
- `jq empty` exits with `0`.

---

### Scenario 3: Large File Size Threshold for Hashing
Test the `--max-hash-size` flag to ensure multi-gigabyte video files do not block the extraction pipeline:

```bash
# Run balanced mode with a 50MB hash threshold
./target/release/metadata-extractor /spsa/temp --max-hash-size 50
```

**Expected Outcome**:
- Video files and large RAW images exceeding 50 MB extract duration, codec, and camera metadata quickly without streaming multi-gigabyte payloads for bitwise hashing.
- Moving-average throughput remains consistently high without multi-minute stalls on individual files.

---

### Scenario 4: Non-Interactive / Script Output Mode
Verify that redirected `stderr` produces discrete single-line periodic updates without ANSI cursor escape sequences:

```bash
# Pipe stderr to a log file
./target/release/metadata-extractor /spsa/temp --fast 2> /tmp/scan_progress.log > /dev/null

# Inspect the log file for clean, periodic lines
head -n 10 /tmp/scan_progress.log
```

**Expected Outcome**:
- The log file contains discrete timestamped progress lines emitted every 5 seconds.
- Zero carriage returns (`\r`) or ANSI cursor sequences (`\x1b[...]`) in `/tmp/scan_progress.log`.

---

### Scenario 5: Early Cancellation (Ctrl+C)
Verify that interrupting an active scan prints a clean partial summary rather than terminating abruptly:

```bash
# Start a scan and send SIGINT after 3 seconds
./target/release/metadata-extractor /spsa/temp &
PID=$!
sleep 3
kill -INT $PID
wait $PID
```

**Expected Outcome**:
- The process catches SIGINT, shuts down background workers cleanly, and emits a partial `Execution Summary` marked with `Status: Cancelled early`.
- Process exits with standard interrupt code `130`.
