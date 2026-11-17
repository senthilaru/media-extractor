# Quickstart: Unified Organize Pipeline

## Verification Scenario 1: Preview Entire Organization (Dry-Run)

```bash
cargo run -- organize --source /Volumes/SP-1/SPSA/unorganized --config drives.yaml --dry-run
```
- Previews extraction, classification, ingestion destinations, replica fanout, and empty directory pruning.
- 0 bytes modified or deleted.

## Verification Scenario 2: Live End-to-End Organization

```bash
cargo run -- organize --source /Volumes/SP-1/SPSA/unorganized --config drives.yaml
```
- Performs extraction -> dedup audit -> primary ingest -> replica sync -> prune & clean empty folders up to drive ceiling.
- Displays live progress per stage and a final summary dashboard.

## Verification Scenario 3: Preserve Source Directory Container

```bash
cargo run -- organize --source /Volumes/SP-1/SPSA/unorganized/Photos --config drives.yaml --keep-source-dir
```
- Ingests and prunes duplicates inside `Photos`, but preserves the `Photos` directory container itself.
