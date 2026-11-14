# Data Model: Unified Organize Pipeline

## Entities

### `OrganizePipelineOptions`
- `source_path`: `PathBuf` (path to scan and clean)
- `config_path`: `Option<PathBuf>` (path to drives.yaml)
- `dry_run`: `bool` (simulate all stages)
- `skip_prune`: `bool` (ingest & replicate, but skip pruning)
- `no_verify_hash`: `bool` (skip destination verification)
- `buffer_size`: `String` (I/O buffer, default "8M")
- `recheck_disk_hash`: `bool`
- `batch_size`: `usize` (extraction batch size, default 500)
- `quiet`: `bool`

### `OrganizePipelineReport`
- `source_path`: `PathBuf`
- `is_dry_run`: `bool`
- `files_scanned`: `usize`
- `files_extracted`: `usize`
- `canonical_masters_identified`: `usize`
- `replicas_preserved`: `usize`
- `files_ingested`: `usize`
- `files_replicated`: `usize`
- `files_pruned`: `usize`
- `empty_dirs_removed`: `usize`
- `bytes_ingested`: `u64`
- `bytes_replicated`: `u64`
- `bytes_reclaimed`: `u64`
- `total_elapsed_seconds`: `f64`
- `stage_elapsed_seconds`: `Vec<(String, f64)>`
- `errors`: `Vec<String>`
