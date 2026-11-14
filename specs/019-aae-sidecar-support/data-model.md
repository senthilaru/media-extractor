# Data Model: Apple AAE Sidecar File Support

## Entities & Value Objects

### 1. `MediaCategory` (Extension)
Location: `src/domain/models.rs`

Existing enum augmented with `Sidecar`:
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaCategory {
    Photo,
    Video,
    RawPhoto,
    Sidecar,
}
```

Display string: `"Sidecar"`.

---

### 2. `SidecarCompanionPair`
Location: `src/domain/dedup_models.rs`

Represents an identified pairing between a primary media file and an optional companion sidecar file:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidecarCompanionPair {
    /// Absolute path to primary media file (photo/video)
    pub primary_path: PathBuf,
    /// Absolute path to companion sidecar file (if detected)
    pub sidecar_path: Option<PathBuf>,
    /// Extension of sidecar (e.g. "aae")
    pub sidecar_extension: Option<String>,
}
```

---

### 3. `SidecarTransferResult`
Location: `src/domain/dedup_models.rs`

Execution outcome of a sidecar companion transfer:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidecarTransferResult {
    /// Whether a companion sidecar was found
    pub found: bool,
    /// Source sidecar path
    pub source_path: Option<PathBuf>,
    /// Destination sidecar path
    pub target_path: Option<PathBuf>,
    /// Bytes transferred
    pub bytes_transferred: u64,
    /// Whether transfer was skipped (identical file already present)
    pub skipped_identical: bool,
    /// Error message if transfer failed
    pub error: Option<String>,
}
```

---

### 4. `SidecarPruneResult`
Location: `src/domain/dedup_models.rs`

Record of sidecars deleted during pruning:
```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SidecarPruneResult {
    /// Number of companion sidecar files deleted
    pub sidecars_deleted: usize,
    /// Bytes reclaimed from sidecars
    pub bytes_reclaimed: u64,
    /// Any errors encountered
    pub errors: Vec<String>,
}
```
