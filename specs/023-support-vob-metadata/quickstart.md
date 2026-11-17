# Quickstart: VOB and DVD Metadata Support

## Verification Scenario 1: Scan & Discover VOB and DVD Metadata

1. Prepare a directory with DVD files:
   - `VTS_01_1.VOB`
   - `VTS_01_0.IFO`
   - `VTS_01_0.BUP`
2. Run scan command:
   ```bash
   cargo run -- /path/to/dvd_folder
   ```
3. Verify output includes:
   - `VTS_01_1.VOB` as Video
   - `VTS_01_0.IFO` as Sidecar
   - `VTS_01_0.BUP` as Sidecar

## Verification Scenario 2: End-to-End Master Ingest & Co-Location

1. Execute master ingestion:
   ```bash
   cargo run -- ingest -c drives.yaml
   ```
2. Inspect the master archive `<Year>/<DD-Mon-YYYY>/Videos/`:
   - `VTS_01_1.VOB` exists in `Videos/`.
   - `VTS_01_0.IFO` and `VTS_01_0.BUP` exist in `Videos/` alongside the video.

## Verification Scenario 3: Automated Test Suite

Run unit and integration tests:
```bash
cargo test --test test_vob_dvd_metadata
```
