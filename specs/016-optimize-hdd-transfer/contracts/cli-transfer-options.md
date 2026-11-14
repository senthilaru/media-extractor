# Contract: CLI Transfer & Buffer Options

**Feature**: `016-optimize-hdd-transfer`
**Type**: Command-Line Interface Contract

## 1. Parameters for `ingest` and `sync-replicas`

### `--buffer-size <SIZE>`
- **Type**: String
- **Default**: `"8M"`
- **Supported Formats**:
  - Megabytes: `4M`, `8M`, `16M`, `4MB`, `8MB`
  - Kilobytes: `512K`, `1024K`, `4096K`
  - Raw bytes: `8388608`
- **Range Constraints**: `64KB` minimum, `32MB` maximum. Values outside this range will be rejected with an informative error message:
  ```text
  Error: Buffer size '64M' exceeds maximum allowed limit of 32MB.
  ```

### `--recheck-disk-hash`
- **Type**: Boolean flag
- **Default**: `false`
- **Description**: When set, performs a secondary read pass across the destination physical media after writing to recalculate the cryptographic hash from disk. By default, verification is performed in-flight in memory, avoiding redundant disk read passes.

### `--no-verify-hash`
- **Type**: Boolean flag
- **Default**: `false`
- **Description**: Skips both in-flight and post-copy cryptographic hash verification, verifying transfers solely via stream byte size parity.

---

## 2. Examples

```bash
# Standard Ingest with 8MB Buffer and In-Flight Verification
cargo run -- ingest --source-drive /Volumes/SP-1/SPSA/unorganized/Photos/Feb2026 --config drives.yaml

# Ingest with Custom 16MB Buffer
cargo run -- ingest --source-drive /Volumes/SP-1/SPSA/unorganized/Photos/Feb2026 --config drives.yaml --buffer-size 16M

# Replica Sync with Custom Buffer Size
cargo run -- sync-replicas -c drives.yaml --buffer-size 8M
```
