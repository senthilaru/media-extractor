# Contract: Perceptual Fingerprinting Ports & Traits (Hexagonal Architecture)

**Feature**: `003-perceptual-deduplication`  
**Date**: 2026-11-07  
**Status**: Revised (Scope Simplified per User Request)  

---

## 1. Domain Ports (Traits)

### 1.1 MediaFingerprinter Port

```rust
use std::path::Path;

use crate::domain::dedup_models::{FingerprintConfig, MediaFingerprint};
use crate::domain::errors::FingerprintError;
use crate::domain::metadata_models::MediaMetadataDocument;
use crate::domain::models::DiscoveredMediaItem;

/// Port for computing cryptographic and perceptual content fingerprints.
pub trait MediaFingerprinter: Send + Sync {
    /// Computes full fingerprint (BLAKE3 + DCT pHash) for a single media item.
    fn fingerprint_file(
        &self,
        item: &DiscoveredMediaItem,
        metadata: Option<&MediaMetadataDocument>,
        config: &FingerprintConfig,
    ) -> Result<MediaFingerprint, FingerprintError>;

    /// Computes fast sample digest (header + trailer sample) without reading full file.
    fn compute_sample_digest(&self, path: &Path) -> Result<String, FingerprintError>;

    /// Computes exact streaming cryptographic digest across file payload.
    fn compute_cryptographic_hash(&self, path: &Path) -> Result<String, FingerprintError>;

    /// Computes 64-bit DCT perceptual hash on an image or RAW preview.
    fn compute_perceptual_hash(&self, path: &Path) -> Result<Option<u64>, FingerprintError>;
}
```

---

## 2. CLI Invocation Contracts

```bash
# Extract single media file metadata with embedded fingerprint:
metadata-extractor extract /Volumes/SP-1/DCIM/100CANON/IMG_1042.CR3

# Stream scanned files directly into metadata extraction with fingerprints:
metadata-extractor scan /Volumes/SP-1 --format json -q | \
  metadata-extractor extract-stream --format json > media_metadata.ndjson
```
