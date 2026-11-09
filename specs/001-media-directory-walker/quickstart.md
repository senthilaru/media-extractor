# Quickstart & Validation Guide: Media Directory Walker

**Feature**: `001-media-directory-walker`  
**Date**: 2026-11-07  
**Status**: Completed  

---

## 1. Prerequisites

- **Rust Toolchain**: Rust 1.82+ with `cargo` and `rustfmt`.
- **Target OS**: macOS (Apple Silicon M1/M2/M3 or Intel) or Linux (x86_64 / arm64).

---

## 2. Setup & Build

Compile the project and check for compiler warnings and formatting:

```bash
# Verify formatting
cargo fmt --check

# Check lints with strict warnings
cargo clippy --all-targets -- -D warnings

# Build optimized release binary
cargo build --release
```

---

## 3. End-to-End Validation Scenarios

### Scenario 1: Directory Traversal on External Hard Disk (e.g. `/Volumes/SP-1`)
Run the scanner against a mounted external hard disk:

```bash
cargo run --release -- scan /Volumes/SP-1 --batch-size 250
```

**Expected Outcome**:
- Traversal begins immediately on the external volume mount and displays live telemetry on `stderr` (e.g., `Throughput: ~20,000 files/sec`).
- Batches of discovered photo/video files are output on `stdout`.
- Non-media files (e.g. `.txt`, `.DS_Store`, volume metadata) are excluded.

### Scenario 2: Streaming JSON Batches from External Hard Drive
Validate machine-readable batch streaming from an external drive mount:

```bash
cargo run --release -- scan /Volumes/SP-1 --format json --batch-size 100 | jq .
```

**Expected Outcome**:
- Output conforms to the NDJSON schema in [contracts/cli-contract.md](contracts/cli-contract.md).
- Each batch message has `type: "batch"`, sequential `batch_sequence`, and valid `items` array.

### Scenario 3: Backpressure Verification
Simulate a slow downstream consumer to verify backpressure and bounded memory:

```bash
# Pipe batches to a slow consumer (1 batch per second)
cargo run --release -- scan /path/to/large/media/folder --format json --batch-size 500 --channel-capacity 10 | \
  while read -r line; do
    echo "Consumer processed batch at $(date)"
    sleep 1
  done
```

**Expected Outcome**:
- Memory usage of the scanner process remains flat (< 50 MB resident set size).
- The scanner pauses reading when channel capacity (10 batches) is reached, resuming when the consumer reads a line.

### Scenario 4: Fault Isolation & Symlink Cycle Guard
Verify handling of circular symlinks and restricted folders:

```bash
# Prepare test fixture with loop
mkdir -p /tmp/scan-test/nested
ln -s /tmp/scan-test /tmp/scan-test/nested/loop_link

# Run scanner
cargo run --release -- scan /tmp/scan-test --follow-symlinks
```

**Expected Outcome**:
- Scanner detects circular link, logs error of kind `CircularSymlink` on `stderr`, and terminates traversal cleanly without infinite recursion.

---

## 4. Running Automated Tests & Benchmarks

```bash
# Run unit tests
cargo test --test unit

# Run integration tests
cargo test --test integration

# Run Criterion micro-benchmarks
cargo bench --bench scanner_bench
```
