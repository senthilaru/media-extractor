# Quickstart Validation: Apple AAE Sidecar Support

## Scenario 1: Automatic Ingestion of Companion AAE Sidecars
1. Place a photo and its sidecar in an unorganized folder:
   - `unorganized/IMG_1001.HEIC`
   - `unorganized/IMG_1001.AAE`
2. Run ingestion:
   ```bash
   cargo run -- ingest --source-drive unorganized --config drives.yaml
   ```
3. Verify that both files are present in the primary master directory:
   - `media/master/<Year>/<Date>/Photos/IMG_1001.HEIC`
   - `media/master/<Year>/<Date>/Photos/IMG_1001.AAE`

## Scenario 2: Parity Replication of AAE Sidecar to Replica
1. Run replica sync:
   ```bash
   cargo run -- sync-replicas -c drives.yaml
   ```
2. Verify replica drive receives `IMG_1001.AAE` alongside `IMG_1001.HEIC`.

## Scenario 3: Pruning Source AAE Sidecar Files
1. Run pruning on the source directory:
   ```bash
   cargo run -- dedup prune --drive unorganized --execute
   ```
2. Verify `unorganized/IMG_1001.AAE` is pruned and empty parent directories are deleted.
