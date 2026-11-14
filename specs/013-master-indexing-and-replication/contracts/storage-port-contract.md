# Storage Port Contract: Master Ingestion and Replication Methods

## 1. `MetadataStorageSink::link_canonical_masters`

```rust
#[async_trait::async_trait]
pub trait MetadataStorageSink: Send + Sync {
    /// Bulk updates all non-canonical duplicate records sharing the given cryptographic hashes
    /// to link to their authoritative canonical master ID and filesystem path.
    ///
    /// If any duplicate record was in `NeedsIngestToMaster`, its status transitions to `MarkForRemoval`.
    async fn link_canonical_masters(
        &self,
        links: &[(String, uuid::Uuid, std::path::PathBuf)],
    ) -> Result<usize, StorageError>;

    /// Updates the replication tracking state for a canonical master document.
    async fn update_master_replication_state(
        &self,
        master_id: uuid::Uuid,
        replication_state: &MasterReplicationState,
    ) -> Result<(), StorageError>;

    /// Queries all canonical master documents matching a specific replication status.
    async fn query_masters_by_replication_status(
        &self,
        status: ReplicationStatus,
    ) -> Result<Vec<MediaMetadataDocument>, StorageError>;
}
```

### Invariant Rules
- **Non-Master Protection**: `link_canonical_masters` MUST NOT alter documents where `dedup.status == CanonicalMaster`.
- **Idempotence**: Running `link_canonical_masters` multiple times with the same hash and master parameters yields identical final state.
- **Atomic Drive Transition**: Sibling records with `NeedsIngestToMaster` automatically transition to `MarkForRemoval` since the asset is now anchored on the primary master volume.
