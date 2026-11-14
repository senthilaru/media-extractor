# Contract: Unified File Transfer Service Interface

**Feature**: `016-optimize-hdd-transfer`
**Type**: Internal Service / API Contract

## 1. Updated `UnifiedFileTransferService` Signature

```rust
impl UnifiedFileTransferService {
    /// Copies data from `source` to `target` using the specified buffer configuration,
    /// computing in-flight cryptographic verification and committing dirty pages with `sync_all`.
    pub async fn stream_copy_verified(
        source: &Path,
        target: &Path,
        expected_size: u64,
        expected_hash: Option<&str>,
        config: TransferBufferConfig,
    ) -> Result<u64, DeduplicationError>;
}
```

## 2. Behavioral Guarantees

1. **Dynamic Bounded Allocation**:
   The allocated chunk buffer size is:
   $$\text{chunk\_size} = \min(\text{expected\_size}, \text{config.buffer\_size\_bytes})$$
   A 200 KB image allocates a 200 KB buffer, while a 2.5 GB video allocates an 8 MB buffer.
2. **In-Flight Cryptographic Computation**:
   As chunks are read from `source` into the memory buffer, `blake3::Hasher::update(&chunk[..bytes_read])` is invoked prior to writing to `target`.
3. **Storage Commit Guarantee (`fsync`)**:
   `dst_file.sync_all().await` is executed before closing write handles.
4. **Immediate Handle Drop**:
   `drop(dst_file)` and `drop(src_file)` release file descriptors immediately after flush.
5. **Rollback on Mismatch**:
   If either in-flight digest verification fails or byte length does not match `expected_size`, `tokio::fs::remove_file(target)` is called immediately and `DeduplicationError::TransferFailed` is returned.
