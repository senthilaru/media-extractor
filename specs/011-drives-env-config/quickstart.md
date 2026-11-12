# Quickstart: Testing Sample Drive Configuration and Environment Variables

## Scenario 1: Validate Sample `drives.example.yaml`

1. Copy sample configuration:
   ```bash
   cp drives.example.yaml drives.yaml
   ```
2. Verify that configuration parses and validates cleanly:
   ```bash
   cargo test --test unit drives_config_tests
   ```

---

## Scenario 2: Test Pure `.env` Drive Configuration (Without YAML File)

1. Set drive environment variables in `.env` or current shell:
   ```bash
   export DRIVE_PRIMARY_MOUNT="/tmp/test_primary"
   export DRIVE_PRIMARY_MASTER="/tmp/test_primary/SPSA/media/master"
   export DRIVE_REPLICA1_MOUNT="/tmp/test_replica1"
   export DRIVE_REPLICA1_MASTER="/tmp/test_replica1/SPSA/media/master"
   ```
2. Run deduplication or ingest preview without `--config`:
   ```bash
   ./target/debug/metadata-extractor dedup --dry-run
   ```
3. Verify that the primary and replica1 drives are recognized and reported in telemetry.

---

## Scenario 3: Test Environment Variable Interpolation in YAML

1. Use placeholder syntax in a YAML file:
   ```yaml
   version: "1.0"
   groups:
     primary: { priority: 100 }
   drives:
     - id: "primary"
       group: "primary"
       mount_path: "${CUSTOM_MOUNT:-/default/path}"
   ```
2. Load configuration with `CUSTOM_MOUNT=/custom/path` set; verify that `mount_path` resolves to `/custom/path`.
3. Load configuration with `CUSTOM_MOUNT` unset; verify that `mount_path` resolves to `/default/path`.
