# Contract: Storage Pool & Capacity Spillover Interface

## Interface Requirements

### 1. `DriveConfiguration` Pool Extensions

```rust
impl DriveConfiguration {
    /// Returns all member volumes belonging to the primary master storage pool.
    pub fn get_primary_pool(&self) -> Vec<&DriveDefinition>;

    /// Returns all member volumes belonging to a given replica storage group.
    pub fn get_replica_pool(&self, replica_group: &str) -> Vec<&DriveDefinition>;

    /// Returns all unique storage group names defined in the configuration.
    pub fn get_storage_groups(&self) -> Vec<&str>;
}
```

### 2. `MasterIngestService` Capacity Spillover Resolution

```rust
impl MasterIngestService {
    /// Resolves the optimal target volume within a storage pool for a file of `file_size_bytes`.
    /// Evaluates volumes in pool priority sequence; spills over to the next volume if available
    /// disk space is less than `file_size_bytes + min_free_space_buffer`.
    pub fn resolve_pool_target_volume<'a>(
        &self,
        pool_volumes: &[&'a DriveDefinition],
        file_size_bytes: u64,
        probe: &dyn DiskSpaceProbe,
        min_free_space_buffer: u64,
    ) -> Result<&'a DriveDefinition, DeduplicationError>;
}
```

### 3. `ReplicaSyncService` Multi-Volume Resolution

```rust
impl ReplicaSyncService {
    /// Checks if a canonical master is already mirrored on ANY mounted volume of the replica pool.
    pub fn is_replicated_in_pool(
        master_doc: &MediaMetadataDocument,
        replica_volumes: &[&DriveDefinition],
    ) -> Option<PathBuf>;
}
```
