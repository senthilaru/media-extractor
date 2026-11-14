# Phase 0 Research: Master Indexing and Replication Lifecycle Tracking

## Technical Decisions

### Decision 1: Replication State Schema in DeduplicationMetadata
- **Decision**: Add an optional `MasterReplicationState` struct to `DeduplicationMetadata` (`replication: Option<MasterReplicationState>`).
- **Rationale**:
  - Embedding replication status directly within `DeduplicationMetadata` maintains domain consistency and avoids requiring a separate collection or join table.
  - Non-master records (e.g. `MarkForRemoval`, `Removed`) can omit this field (`None`), keeping document size minimal.
  - Allows atomic updates in MongoDB when master ingestion and replica synchronization occur.
- **Alternatives Considered**:
  - *Separate `replications` collection*: Adds multi-collection query overhead and eventual consistency risks without transactional benefits for single-master architectures.
  - *Flat string status field (`replication_status: String`)*: Does not capture which specific replica drives have completed vs. which are still pending in a multi-replica setup (e.g., `replica1` succeeded, `replica2` offline).

### Decision 2: Replica Drive Discovery from `DriveConfiguration`
- **Decision**: Automatically discover all candidate replica drives from `DriveConfiguration` (all drives where `group` starts with `"replica"` or `rules.is_backup_target == true`).
- **Rationale**:
  - `drives.yaml` already classifies drive groups (e.g., `primary`, `replica1`, `replica2`).
  - When an asset is ingested into the primary master, `MasterIngestService` evaluates the configured replica targets and populates `pending_drives` with all active or configured replica drive IDs.
  - If no replica drives are configured, `replication.status` immediately evaluates to `ReplicationStatus::Replicated`.
- **Alternatives Considered**:
  - *Hardcoding replica drive IDs*: Violates configuration decoupling principles.
  - *Requiring replica drives to be mounted during ingest*: Violates operational flexibility; replica drives may be rotated or offline during initial primary ingestion.

### Decision 3: Cluster-Wide Master Linking on Ingestion
- **Decision**: Upon successful master file transfer (or `SkippedIdentical` match), execute `sink.link_canonical_masters` to update all documents matching `fingerprint.cryptographic_hash` where `status != CanonicalMaster`.
- **Rationale**:
  - In a duplicate cluster (e.g., 5 copies of `lv_0_20260724194130.mp4` with different names), exactly 1 copy is transferred to the primary master.
  - The newly created `CanonicalMaster` document receives the authoritative primary file path and a unique master UUID.
  - Linking all sibling copies in MongoDB immediately to this master UUID and master path ensures that subsequent `dedup prune` commands can safely verify the master on disk and delete all redundant files without skipping unverified masters.
- **Alternatives Considered**:
  - *Requiring a second `dedup --all` pass*: Creates unnecessary friction and confusion for users who expect `ingest` to leave the catalog in a consistent state.

### Decision 4: Replication Synchronization Integration (`MultiReplicaSyncService`)
- **Decision**: When `MultiReplicaSyncService::execute_sync` runs (or when `--replicate` is passed to `ingest`), successful transfers to replica drives:
  1. Register the replica document with `dedup.status = ReplicaKeep`.
  2. Update the corresponding `CanonicalMaster` document's `replication.replicated_drives` and `replication.pending_drives`.
  3. If `pending_drives` becomes empty, set `replication.status = ReplicationStatus::Replicated`; otherwise `ReplicationStatus::PartiallyReplicated`.
- **Rationale**:
  - Closes the lifecycle loop: master knows which replicas have received the file, and replica documents link back to the canonical master ID.
- **Alternatives Considered**:
  - *Inferring replication dynamically on every query*: Slows down status reporting with multiple aggregation pipelines across millions of records.
