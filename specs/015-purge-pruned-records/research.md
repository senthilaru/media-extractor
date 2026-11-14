# Phase 0 Research: Purge Obsolete Database Records on Duplicate Prune

## Technical Decisions

### Decision 1: Hard Deletion in MongoDB for Pruned Duplicate Records
- **Decision**: Introduce `delete_document(doc_id: Uuid)` and `delete_documents(doc_ids: &[Uuid])` on `MetadataStorageSink` and `MongoDbStorageSink`, and invoke them upon verified pruning.
- **Rationale**:
  - Once redundant duplicate files have been confirmed to exist safely in the primary canonical master and their physical duplicates are pruned, keeping obsolete records adds database clutter, index overhead, and confusion in search queries.
  - The CLI already saves a pre-deletion manifest via `--manifest-out <PATH>` if audit history is required.
- **Alternatives Considered**:
  - *Retaining `status: "Removed"` indefinitely*: Rejected by user request. Bloats the database and creates confusion.
  - *Separate `purge` CLI command*: While a dedicated purge command could exist, executing hard deletion directly during `dedup prune --execute` provides the cleanest, most immediate operator experience.

### Decision 2: Purging Pre-Existing "Removed" Records on Target Drive
- **Decision**: During `dedup prune --drive <path> --execute`, query and delete any existing records for the target drive that already have status `Removed` from prior runs.
- **Rationale**:
  - Users who ran `dedup prune --execute` in previous versions have records currently stuck in the `Removed` state.
  - Purging them during prune execution ensures the database is completely cleansed of obsolete entries for that drive.
- **Alternatives Considered**:
  - *Leaving historical `Removed` records*: Leaves obsolete residue in the database.

### Decision 3: Safety Gate Against Deleting Canonical Masters or Replicas
- **Decision**: Before executing any document deletion in MongoDB, check that the document does NOT have `dedup.status == CanonicalMaster` or `dedup.status == ReplicaKeep`.
- **Rationale**:
  - Defense-in-depth: guarantees that database records representing canonical masters or replica backups are impossible to delete during duplicate pruning operations.
- **Alternatives Considered**:
  - *Blind delete by ID*: Risky if an invalid document ID is passed.

### Decision 4: Telemetry Reporting in `PruneExecutionReport`
- **Decision**: Add `records_purged: usize` to `PruneExecutionReport` and display it in terminal summary output.
- **Rationale**:
  - Observability principle: gives operators clear confirmation of both physical files deleted from disk and metadata records purged from the database.
