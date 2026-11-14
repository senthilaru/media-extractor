# Research & Technical Decisions: Unified Organize Pipeline

## Decision 1: Domain Service Orchestration vs CLI Subprocess Spawning

### Decision
Implement `OrganizePipelineService` in Rust as an application-layer service that orchestrates `MetadataExtractionEngine`, `MediaDeduplicationEngine`, `MasterIngestService`, `ReplicaService`, and `DrivePruningService` directly in-process, sharing a single MongoDB connection and config instance.

### Rationale
- Subprocess spawning (`cargo run` 5 times) incurs process startup overhead, multiple cargo rebuild checks, and 5 separate database connection pool setups.
- In-process orchestration allows shared memory structures, instant stage transitions, zero database reconnect overhead, and unified error handling.

---

## Decision 2: Stage Gating and Fail-Fast Safety

### Decision
Each stage returns a typed result report. If any stage returns an unrecoverable `Err`, the pipeline immediately halts and displays the error. Specifically, Stage 5 (`DrivePruningService::prune_drive`) only executes if:
1. `!options.skip_prune`
2. Stages 1-4 completed without fatal aborts.
3. The source directory is strictly validated against `ArchiveProtectionGuard`.

### Rationale
Prevents accidental deletion of source files if ingestion or replication failed or was incomplete.

---

## Decision 3: CLI Subcommand Flags & Options

### Decision
Expose the subcommand as `organize` with aliases `archive`, `pipeline`.
Options:
- `-s, --source <PATH>`: Source drive or directory
- `-c, --config <PATH>`: Drive YAML configuration (default `drives.yaml`)
- `--dry-run`: Previews all 5 stages
- `--skip-prune`: Runs extraction, dedup, ingestion, replication, but leaves source files intact
- `--no-verify-hash`: Skips BLAKE3 re-hashing during ingest if in-flight verification is sufficient
- `--buffer-size <SIZE>`: Default 8M
- `--recheck-disk-hash`: Force secondary disk re-read
