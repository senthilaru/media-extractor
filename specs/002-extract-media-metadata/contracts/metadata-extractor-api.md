# Contract: Metadata Extractor Public API & Traits

**Feature**: `002-extract-media-metadata`  
**Date**: 2026-11-07  
**Status**: Completed  

---

## 1. Domain Ports (Traits)

Following Hexagonal Architecture, metadata extraction is abstracted behind clean domain traits.

```rust
use std::path::Path;
use crate::domain::models::{DiscoveredMediaItem, DiscoveryBatch};
use crate::domain::metadata_models::MediaMetadataDocument;
use crate::domain::errors::ExtractionError;

/// Core port for extracting metadata from a single media file.
pub trait MediaMetadataExtractor: Send + Sync {
    /// Extracts rich metadata from a single media file.
    ///
    /// Accepts the discovered media item (path, size, category) and optional scanned root directory.
    fn extract_file(
        &self,
        item: &DiscoveredMediaItem,
        scanned_root: Option<&Path>,
    ) -> Result<MediaMetadataDocument, ExtractionError>;
}

/// Port for batch metadata extraction orchestrators.
#[async_trait::async_trait]
pub trait BatchMetadataExtractor: Send + Sync {
    /// Processes a batch of discovered items in parallel across CPU worker threads,
    /// returning normalized metadata documents.
    async fn extract_batch(
        &self,
        batch: DiscoveryBatch,
        scanned_root: Option<&Path>,
    ) -> Vec<MediaMetadataDocument>;
}
```

---

## 2. Extraction Error Definitions

```rust
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ExtractionError {
    #[error("File not found: '{0}'")]
    FileNotFound(PathBuf),

    #[error("I/O error reading '{path}': {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("Corrupted or malformed header in '{path}': {message}")]
    CorruptHeader {
        path: PathBuf,
        message: String,
    },

    #[error("Unsupported media format for extraction: '{0}'")]
    UnsupportedFormat(String),
}
```
