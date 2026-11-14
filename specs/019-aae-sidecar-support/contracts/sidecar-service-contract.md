# Contract: Apple AAE Sidecar Management

## 1. Companion Discovery Contract

```rust
pub trait SidecarDetector: Send + Sync {
    /// Detects if an .AAE sidecar companion exists on disk for a given media file path.
    /// Checks:
    /// - <stem>.AAE
    /// - <stem>.aae
    /// - <stem>.Aae
    /// - Standard Apple edit naming: if stem is `IMG_E1234`, also checks `IMG_1234.AAE`
    fn find_companion_sidecar(&self, media_path: &Path) -> Option<PathBuf>;
}
```

## 2. Ingest Transfer Contract

When `TransferService::execute_transfer` or `MasterIngestService` ingests a primary media asset:
1. `find_companion_sidecar(source_path)` is called.
2. If a companion `.AAE` file is found:
   - Compute destination sidecar path: `target_media_path.with_extension(sidecar_ext)`.
   - Perform verified stream copy with buffer configuration.
   - If `delete_source: true`, the source `.AAE` file is also removed only after verified copy.

## 3. Replica Synchronization Contract

When `ReplicaSyncService` replicates a media asset from primary master to replica drive:
1. Check if `primary_file_path.with_extension("AAE")` (or case-insensitive) exists.
2. If found, stream-copy to replica drive at `replica_target_path.with_extension("AAE")`.
3. Verify cryptographic hash.

## 4. Pruning Contract

When `DrivePruningService::prune_drive` prunes a source media file:
1. Check if companion `.AAE` file exists in the source directory.
2. Check if the matching `.AAE` file exists on the primary master drive.
3. If confirmed present in master, delete the source `.AAE` file and track metrics (`bytes_reclaimed`, `sidecars_deleted`).
