# Data Model: Multi-Volume Storage Extension Pools

## Entities & Configurations

### 1. Storage Pool Concept (`StoragePool`)

Logical grouping of `DriveDefinition` instances sharing a group role:

```rust
pub struct StoragePool<'a> {
    pub group_id: &'a str,
    pub priority: u32,
    pub volumes: Vec<&'a DriveDefinition>,
}

impl<'a> StoragePool<'a> {
    pub fn is_primary(&self) -> bool;
    pub fn is_replica(&self) -> bool;
}
```

### 2. Drive Group Specifications in `drives.yaml`

Multiple drives in `drives: Vec<DriveDefinition>` sharing the same `group`:

```yaml
version: "1.0"

groups:
  primary:
    priority: 100
    description: "Primary Master Storage Pool"
  replica1:
    priority: 80
    description: "First Backup Replica Pool"
  replica2:
    priority: 70
    description: "Second Backup Replica Pool"

drives:
  # Primary Storage Pool (Member Volumes)
  - id: "primary_vol1"
    group: "primary"
    mount_path: "/Volumes/SP-1"
    master_path: "/Volumes/SP-1/SPSA/media/master"

  - id: "primary_vol2"
    group: "primary"
    mount_path: "/Volumes/SP-3"
    master_path: "/Volumes/SP-3/SPSA/media/master"

  # Replica 1 Storage Pool (Member Volumes)
  - id: "replica_1_vol1"
    group: "replica1"
    mount_path: "/Volumes/SP-2"
    master_path: "/Volumes/SP-2/SPSA/media/master"

  - id: "replica_1_vol2"
    group: "replica1"
    mount_path: "/Volumes/SP-4"
    master_path: "/Volumes/SP-4/SPSA/media/master"
```

### 3. Capacity Probe Abstraction (`DiskSpaceProbe`)

```rust
pub trait DiskSpaceProbe: Send + Sync {
    /// Returns available bytes on the filesystem hosting the given path.
    fn available_bytes(&self, path: &Path) -> Result<u64, std::io::Error>;
}

/// POSIX implementation using statvfs.
#[derive(Debug, Default, Clone)]
pub struct PosixDiskSpaceProbe;

impl DiskSpaceProbe for PosixDiskSpaceProbe {
    fn available_bytes(&self, path: &Path) -> Result<u64, std::io::Error> {
        // Calls libc::statvfs
    }
}
```

### 4. Intra-Pool Single-Copy Lifecycle Transitions

Within any storage group (`primary`, `replica1`, `replica2`):
- If `count(group) == 1`:
  - For `primary`: Marked `DeduplicationStatus::CanonicalMaster`.
  - For `replicaX`: Marked `DeduplicationStatus::ReplicaKeep`.
- If `count(group) > 1` (duplicate files residing on multiple member volumes of the same group):
  - Exactly one copy on the preferred volume (e.g. `vol1` over `vol2`) is marked `CanonicalMaster` (for primary) or `ReplicaKeep` (for replica).
  - All other redundant intra-pool copies are marked `DeduplicationStatus::MarkForRemoval`.
  - `canonical_master_id` and `canonical_master_path` reference the keeper.
  - `reclaimable_bytes` equals the duplicate file size.
