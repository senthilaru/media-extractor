# Quickstart: Resolution-Aware Deduplication Quality Validation

## Prerequisites
- Rust toolchain (`cargo`, `rustc`)
- Local or test MongoDB instance

## Running Unit & Integration Tests

### 1. Test Quality Ranking Unit Logic
Run unit tests verifying that higher resolution always beats lower resolution, and larger size breaks ties:
```bash
cargo test --test unit test_resolution_quality_ranking
```

### 2. Test Content Group Deduplication with Mixed Resolutions
Run integration test verifying that in a cluster with 4000x3000, 1920x1080, and 300x300 images:
- The 4000x3000 image is marked `NeedsIngestToMaster` (or `CanonicalMaster`)
- The 1920x1080 and 300x300 images are marked `MarkForRemoval`
```bash
cargo test --test integration test_mixed_resolution_content_group_evaluation
```

### 3. End-to-End Organize Dry Run
Run organize in dry-run mode to observe resolution-aware classification in action across catalog drives:
```bash
cargo run -- organize -c drives.yaml --dry-run
```
Expected output:
- Low-resolution previews and smaller duplicates are assigned `MarkForRemoval`
- High-resolution originals are assigned `NeedsIngestToMaster` or `CanonicalMaster`
