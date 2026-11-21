# Research: Multi-Volume Storage Extension Pools

## Technical Decisions

### 1. Storage Pool Abstraction & Drive Grouping

- **Decision**:
  - A storage pool is represented by multiple `DriveDefinition` entries sharing the same `group` identifier (e.g. `group: "primary"`, `group: "replica1"`, `group: "replica2"`).
  - Member volumes within a pool are ordered by configuration declaration order or volume index (`primary_vol1` -> `primary_vol2` -> `primary_vol3`).
  - All volumes in `group: "primary"` inherit the group's maximum retention priority (100).
- **Rationale**:
  - Reuses the existing `drives.yaml` structure (`drives: Vec<DriveDefinition>`) without introducing breaking schema changes.
  - Allows an arbitrary number of extension hard drives to be added simply by defining new drives under the existing group names.
- **Alternatives Considered**:
  - Adding a nested `pools` object in `drives.yaml`: Rejected because it would break existing configuration files and require migrating all current setups. Sharing `group: "primary"` works cleanly with existing `drives.yaml` and `.env` parsing.

### 2. Single-Copy Invariant & Intra-Pool Deduplication

- **Decision**:
  - The deduplication engine (`MediaDeduplicationEngine`) must evaluate all copies residing on any volume of `group: "primary"`.
  - If multiple bitwise-identical copies are found across primary volumes (e.g., one on `primary_vol1` and one on `primary_vol2`):
    - Retain the copy on the earlier volume (or with higher quality/metadata) as the single `CanonicalMaster`.
    - Mark all other copies on other primary volumes as `MarkForRemoval` with reference to the canonical master.
  - Similarly, for replica pools: at most one copy of an asset is marked `ReplicaKeep` per replica group. If an asset is already present on `replica_1_vol1`, any duplicate on `replica_1_vol2` is marked `MarkForRemoval`.
- **Rationale**:
  - The user explicitly requested: "we can have the folder names as same like 2011, 2026 can be found in both primary and primary extension. but, it should maintain single copy of files in it."
  - Enforcing the single-copy invariant eliminates intra-pool redundancy and reclaims precious storage space on nearly full drives.
- **Alternatives Considered**:
  - Flagging intra-primary duplicates as `ConflictReview`: Rejected because bitwise-identical assets with identical content require no manual user intervention; retaining the primary base copy and marking the redundant copy for removal is deterministic and safe.

### 3. Disk Space Probing & Capacity-Aware Spillover

- **Decision**:
  - Introduce a `DiskSpaceProbe` port trait with a default implementation using POSIX `libc::statvfs` (available on macOS and Linux) to inspect available disk space (`f_bavail * f_frsize`).
  - Configurable safety buffer (default: 5 GB, configurable via `min_free_space_bytes` or `MEDIA_POOL_MIN_FREE_SPACE_GB`).
  - During master ingestion, the system evaluates member volumes of the primary pool in sequence:
    - If `primary_vol1` has sufficient space (`available_space >= file_size + safety_buffer`), write to `primary_vol1`.
    - Otherwise, automatically spill over to the next volume in the pool (`primary_vol2`).
    - If all member volumes in the primary pool are exhausted, halt with `StoragePoolExhausted`.
- **Rationale**:
  - External hard drives experience severe performance degradation and filesystem fragmentation when filled to 100%. A 5 GB safety buffer ensures smooth directory creation, inode allocation, and journaling.
  - Abstracting space checks behind a trait enables fast, 100% deterministic unit and integration tests with simulated disk capacities.
- **Alternatives Considered**:
  - Executing `df -k` via shell subprocess: Rejected due to high process spawning latency on large batches (1000s of items). `libc::statvfs` executes in microseconds with zero allocations.

### 4. Multi-Volume Replica Synchronization Architecture

- **Decision**:
  - `ReplicaSyncService` matches canonical masters against replica groups rather than a single replica drive.
  - When mirroring a canonical master to a replica group:
    - First check if the file already exists on ANY mounted volume in the replica group (e.g. `replica_1_vol1` or `replica_1_vol2`). If found, mark as replicated and skip.
    - If missing from the replica group, select an active volume in the group with sufficient available capacity (following spillover logic) and stream the file.
- **Rationale**:
  - Ensures full 1:1 data parity between the primary pool and each replica backup pool regardless of which physical disk holds which file.
- **Alternatives Considered**:
  - Strict 1:1 volume pairing (`primary_vol1` -> `replica_1_vol1`, `primary_vol2` -> `replica_1_vol2`): Rejected because replica drives may have different physical storage capacities (e.g. 14 TB primary spanning to 8 TB + 6 TB replicas). Pool-level capacity matching provides maximum flexibility.

### 5. Environment Variable Naming Conventions

- **Decision**:
  - Primary pool environment variables:
    - Base: `DRIVE_PRIMARY_MOUNT`, `DRIVE_PRIMARY_MASTER`, `DRIVE_PRIMARY_ID` (defaults to `primary_main` or `primary_vol1`)
    - Extensions: `DRIVE_PRIMARY_EXT1_MOUNT`, `DRIVE_PRIMARY_EXT1_MASTER`, `DRIVE_PRIMARY_EXT1_ID` (or `EXT2`, `EXT3`, etc.)
  - Replica 1 pool:
    - Base: `DRIVE_REPLICA1_MOUNT`, `DRIVE_REPLICA1_MASTER`, `DRIVE_REPLICA1_ID`
    - Extensions: `DRIVE_REPLICA1_EXT1_MOUNT`, `DRIVE_REPLICA1_EXT1_MASTER`, etc.
  - Replica 2 pool:
    - Base: `DRIVE_REPLICA2_MOUNT`, `DRIVE_REPLICA2_MASTER`
    - Extensions: `DRIVE_REPLICA2_EXT1_MOUNT`, `DRIVE_REPLICA2_EXT1_MASTER`, etc.
- **Rationale**:
  - Retains 100% backward compatibility with existing single-drive `.env` configs while allowing instant extension via standard numbered suffixes.
