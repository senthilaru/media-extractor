# Feature Specification: Unified Organize Pipeline Subcommand

**Feature Branch**: `018-unified-organize-pipeline`

**Created**: 2026-11-13

**Status**: Draft

**Input**: User request: Add a native unified command `cargo run -- organize` to execute the full media lifecycle in sequence: scan/extract -> dedup audit -> ingest to master -> sync replicas -> prune duplicates & clean empty directories.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Full End-to-End Media Organization in a Single Command (Priority: P1) 🎯 MVP

As a media archivist with an unorganized collection of photos and videos on an external drive or folder, I want to run a single command (`cargo run -- organize --source <PATH> --config drives.yaml`) that automatically scans metadata into MongoDB, audits duplicates and replicas, ingests unique files into the canonical primary master hierarchy, fans out copies to backup replicas, and safely prunes redundant source files and empty directories, so that I don't have to manually execute five separate CLI commands.

**Why this priority**: Eliminates repetitive multi-step CLI operations, ensures safe dependency chaining without operator error, and unifies database connections and execution telemetry into one seamless workflow.

**Independent Test**: Connect an unorganized media folder and primary/replica drives. Run `cargo run -- organize --source <DIR> --config drives.yaml`. Verify all unique files are placed in primary master, mirrored to replicas, cataloged in MongoDB, and duplicate source files and newly emptied directories are cleanly pruned.

**Acceptance Scenarios**:

1. **Given** an unorganized folder containing 10 photos (8 unique, 2 duplicates of existing canonical masters), **When** `cargo run -- organize --source <DIR>` executes, **Then** all 10 are cataloged in MongoDB, the 8 unique photos are copied to the primary master hierarchy, the 8 new canonical masters are mirrored to active replica drives, the 2 duplicate source files and the 8 ingested source files are pruned, and empty subdirectories are removed.
2. **Given** any error in earlier stages (such as primary drive becoming full or network disconnect), **When** a stage fails, **Then** the pipeline halts immediately and never proceeds to destructive pruning.

---

### User Story 2 - Comprehensive Dry-Run Safety Preview (Priority: P2)

As a safety-first operator, I want to run `cargo run -- organize --source <PATH> --dry-run` to preview all stages—showing how many files will be scanned, how many unique assets will be ingested, how many will be replicated, how much space will be reclaimed, and how many empty directories will be removed—without modifying disk or deleting anything.

**Why this priority**: Users need confidence in what the automated pipeline will do before allowing physical copy and deletion operations on external storage.

**Independent Test**: Run `cargo run -- organize --source <PATH> --dry-run`. Verify telemetry outputs plans for all 5 stages, while disk contents and MongoDB records remain unmodified.

**Acceptance Scenarios**:

1. **Given** an unorganized source path, **When** running with `--dry-run`, **Then** the command simulates extraction, audit, ingestion, replica fanout, and pruning, printing a complete preview summary with zero file deletions and zero database record deletions.

---

### User Story 3 - Selective Stage Flags (Priority: P3)

As an archivist who may only want to ingest and replicate without pruning source files, I want flags such as `--skip-prune` or `--no-verify-hash` so that I have granular control over the automated pipeline.

**Why this priority**: Gives users flexibility when testing new staging drives or retaining source archives temporarily before permanent cleanup.

**Independent Test**: Run `cargo run -- organize --source <DIR> --skip-prune`. Verify files are ingested to primary master and replicated to replicas, but source files are preserved.

**Acceptance Scenarios**:

1. **Given** `--skip-prune` is passed, **When** the pipeline runs, **Then** stages 1 through 4 execute successfully, and stage 5 (pruning) is cleanly bypassed.

---

### Edge Cases

- **Offline Replicas**: If replica drives are unmounted or offline, the pipeline must successfully ingest to primary master, note offline replicas in the replication summary without crashing, and proceed safely.
- **Empty Source Directory**: If the source directory contains no media, the pipeline exits gracefully with a message indicating 0 files found.
- **Protected Path as Source**: If the user accidentally specifies `/Volumes/SP-1/SPSA/media/master` as `--source`, the command must immediately abort with an error to protect the master archive.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST provide a top-level subcommand `organize` (with aliases `archive`, `pipeline`).
- **FR-002**: The `organize` command MUST accept `--source` (`-s`) pointing to a directory or drive path containing unorganized media.
- **FR-003**: The `organize` command MUST accept `--config` (`-c`) defaulting to `drives.yaml`.
- **FR-004**: Stage 1 MUST scan and extract metadata from the source path and upsert records into MongoDB.
- **FR-005**: Stage 2 MUST perform universal deduplication and replica audit, updating `CanonicalMaster`, `ReplicaKeep`, `NeedsIngestToMaster`, and `MarkForRemoval` classifications.
- **FR-006**: Stage 3 MUST ingest all unique `NeedsIngestToMaster` media from the source into the primary master structure `<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}`.
- **FR-007**: Stage 4 MUST synchronize all new or unmirrored canonical masters to all mounted replica drives.
- **FR-008**: Stage 5 MUST prune all verified duplicate and ingested source files from the source directory and recursively remove newly emptied ancestor directories up to the source root.
- **FR-009**: The system MUST abort the pipeline immediately if any stage returns an unrecoverable error, strictly preventing Stage 5 (prune) from executing on failure.
- **FR-010**: The system MUST display an end-to-end audit summary table upon completion reporting counts and byte totals for every stage.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Running a single command `cargo run -- organize --source <PATH>` completes all 5 archiving stages without manual intervention.
- **SC-002**: 100% of newly ingested unique files are verified bitwise and replicated to active replica drives.
- **SC-003**: In `--dry-run` mode, 0 bytes are modified or deleted on disk.
- **SC-004**: If any copy or verification fails, 0 source files are deleted.
- **SC-005**: The entire pipeline shares a single MongoDB connection pool rather than reconnecting per stage.
