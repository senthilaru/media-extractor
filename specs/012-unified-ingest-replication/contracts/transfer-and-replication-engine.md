# Contract: Internal Transfer & Replication Engine API

**Feature Branch**: `012-unified-ingest-replication`  
**Date**: 2026-11-10  
**Status**: Specification

---

## 1. Overview
This contract defines internal Rust traits, structs, and service signatures governing file transfers, verification, collision avoidance, and replica synchronization.

---

## 2. Shared Transfer Component: `UnifiedFileTransferService`

Located in `src/application/transfer_service.rs` (or `src/infrastructure/file_transfer.rs`).

```rust
pub struct UnifiedFileTransferService;

impl UnifiedFileTransferService {
    /// Copies a file using a bounded 1MB asynchronous streaming buffer (`tokio::io`),
    /// verifies byte size and BLAKE3 cryptographic digest, and purges the destination file
    /// if any verification fails.
    pub async fn stream_copy_verified(
        source: &Path,
        target: &Path,
        expected_size: u64,
        expected_hash: Option<&str>,
        verify_hash: bool,
    ) -> Result<u64, DeduplicationError>;

    /// Resolves canonical collision paths without overwriting different content.
    /// Returns Available, IdenticalExists, or CollisionResolved with incremented suffix.
    pub fn resolve_collision_path(
        target_path: &Path,
        source_size: u64,
        source_hash: Option<&str>,
        hasher: &dyn MediaFingerprinter,
    ) -> PathResolution;

    /// Formats calendar components and media categories into a canonical subpath:
    /// `<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}/<Filename>`
    pub fn build_canonical_relative_path(
        year: i32,
        month: u32,
        day: u32,
        category: MediaCategory,
        filename: &str,
    ) -> PathBuf;
}
```

---

## 3. Storage Sink Extension: `MetadataStorageSink`

Extends `src/domain/ports.rs` to support querying all content groups and replica candidates.

```rust
#[async_trait::async_trait]
pub trait MetadataStorageSink: Send + Sync {
    // ... existing methods ...

    /// Queries all content groups grouped by cryptographic hash across the database.
    /// Unlike query_duplicate_groups, this includes unique assets (count == 1)
    /// as well as duplicate clusters (count > 1).
    async fn query_all_content_groups(
        &self,
        mount_prefix: Option<&str>,
    ) -> Result<Vec<(String, Vec<MediaMetadataDocument>)>, StorageError>;

    /// Queries all canonical master assets present in the database.
    async fn query_canonical_masters(
        &self,
    ) -> Result<Vec<MediaMetadataDocument>, StorageError>;

    /// Queries existing replica documents associated with a specific replica drive ID.
    async fn query_replica_documents(
        &self,
        replica_drive_id: &str,
    ) -> Result<Vec<MediaMetadataDocument>, StorageError>;
}
```

---

## 4. Multi-Replica Service: `ReplicaSyncService`

Located in `src/application/replica_service.rs`.

```rust
pub struct ReplicaSyncOptions {
    /// Target specific replica drive ID (None = all configured mounted replicas)
    pub target_replica_id: Option<String>,
    /// Override primary master drive ID
    pub primary_drive_id: Option<String>,
    /// Whether this is a non-mutating preview run
    pub dry_run: bool,
    /// Whether to verify destination BLAKE3 hashes
    pub verify_hash: bool,
}

pub struct ReplicaSyncService;

impl ReplicaSyncService {
    pub fn new() -> Self;

    /// Compiles a ReplicaSyncPlan detailing all missing files on mounted replica drives.
    pub async fn compile_plan(
        &self,
        sink: &dyn MetadataStorageSink,
        config: &DriveConfiguration,
        options: &ReplicaSyncOptions,
    ) -> Result<ReplicaSyncPlan, DeduplicationError>;

    /// Executes the replication plan, copying missing files to replicas, verifying hashes,
    /// inserting ReplicaKeep documents into the database, and returning a detailed report.
    pub async fn execute_sync(
        &self,
        sink: &dyn MetadataStorageSink,
        config: &DriveConfiguration,
        options: ReplicaSyncOptions,
    ) -> Result<ReplicaSyncReport, DeduplicationError>;
}
```

---

## 5. Universal Classification Engine

In `src/application/dedup_engine.rs`:

```rust
impl MediaDeduplicationEngine {
    /// Evaluates any content group (single document or multi-document duplicate cluster)
    /// against drive hierarchies and assigns appropriate DeduplicationStatus.
    pub fn evaluate_content_group(
        docs: &[MediaMetadataDocument],
        matcher: &CanonicalPathMatcher,
    ) -> Vec<(Uuid, DeduplicationMetadata)>;

    /// Evaluates the entire database catalog (both unique assets and duplicate clusters).
    pub async fn run_universal_classification(
        &self,
        sink: &dyn MetadataStorageSink,
        config: &DriveConfiguration,
        dry_run: bool,
    ) -> Result<DeduplicationSummary, DeduplicationError>;
}
```
