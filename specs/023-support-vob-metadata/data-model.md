# Data Model: Support for VOB Video Extensions and DVD Metadata Files

## Entities & Classification

### 1. `MediaCategory` Mapping

Existing enum variants:
```rust
pub enum MediaCategory {
    Photo,
    Video,
    RawPhoto,
    Sidecar,
}
```

- `.vob` maps to `MediaCategory::Video`.
- `.ifo` maps to `MediaCategory::Sidecar`.
- `.bup` maps to `MediaCategory::Sidecar`.

### 2. Sidecar Stem Matching & Grouping

```rust
pub struct SidecarCompanion {
    pub path: PathBuf,
    pub extension: String,
    pub sidecar_type: SidecarType,
}

pub enum SidecarType {
    AppleAae,
    DvdIfo,
    DvdBup,
    Generic,
}
```

### 3. Destination Directory Routing

For any `MediaMetadataDocument` with `media_category: MediaCategory::Sidecar`:
- If companion is found (in batch tasks, canonical masters, or on-disk directory):
  - Co-locate inside the companion asset's parent folder (`Videos/` for `.vob`).
- If no companion is found:
  - If extension is `ifo` or `bup` -> `<Year>/<DD-Mon-YYYY>/Videos/<filename>`
  - If extension is `aae` or other -> `<Year>/<DD-Mon-YYYY>/Photos/<filename>`
