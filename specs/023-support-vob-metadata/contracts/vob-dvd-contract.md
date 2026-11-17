# Contract: VOB and DVD Metadata Companion Interface

## Interface Requirements

### 1. `MediaClassifier`
```rust
fn classify(&self, path: &Path) -> Option<MediaCategory>;
```
- `"vob"` -> `Some(MediaCategory::Video)`
- `"ifo"` -> `Some(MediaCategory::Sidecar)`
- `"bup"` -> `Some(MediaCategory::Sidecar)`

### 2. `UnifiedFileTransferService` Companion Functions
```rust
impl UnifiedFileTransferService {
    /// Detects all companion sidecars (.aae, .ifo, .bup) on disk for a given media file path.
    pub fn find_companion_sidecars(media_path: &Path) -> Vec<PathBuf>;

    /// Detects if a media file stem and sidecar stem match, supporting:
    /// 1. Exact match: m == s
    /// 2. Apple edit variants: IMG_E*, IMG_O*, OIMG_*
    /// 3. DVD Title Set sets: VTS_xx_y matches VTS_xx_0 or VTS_xx_y
    /// 4. DVD Disc sets: VIDEO_TS matches VIDEO_TS
    pub fn is_companion_sidecar_stem_match(media_stem: &str, sidecar_stem: &str, sidecar_ext: &str) -> bool;

    /// Transfers all companion sidecars alongside a primary media asset into its destination folder.
    pub async fn transfer_companion_sidecars_if_present(
        source_media_path: &Path,
        target_media_path: &Path,
        config: TransferBufferConfig,
        delete_source: bool,
    ) -> Result<Vec<SidecarTransferResult>, DeduplicationError>;
}
```
