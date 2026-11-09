use rayon::prelude::*;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};
use uuid::Uuid;

use crate::domain::dedup_models::FingerprintConfig;
use crate::domain::errors::ExtractionError;
use crate::domain::metadata_models::{
    AtomicExtractionCounters, ExtractionMetrics, ExtractionResultKind, ExtractionStatus,
    FileContext, MediaMetadataDocument, TemporalMetadata,
};
use crate::domain::models::{DiscoveredMediaItem, DiscoveryBatch, MediaCategory};
use crate::domain::ports::{BatchMetadataExtractor, MediaFingerprinter, MediaMetadataExtractor};
use crate::infrastructure::blake3_hasher::Blake3MediaHasher;
use crate::infrastructure::exif_parser::ExifMetadataExtractor;
use crate::infrastructure::video_parser::VideoMetadataExtractor;

/// Orchestration engine coordinating format-specific extractors across CPU worker threads.
#[derive(Debug, Clone)]
pub struct MetadataExtractionEngine {
    exif_extractor: Arc<ExifMetadataExtractor>,
    video_extractor: Arc<VideoMetadataExtractor>,
    hasher: Arc<Blake3MediaHasher>,
    counters: Arc<AtomicExtractionCounters>,
}

impl Default for MetadataExtractionEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl MetadataExtractionEngine {
    pub fn new() -> Self {
        Self {
            exif_extractor: Arc::new(ExifMetadataExtractor::new()),
            video_extractor: Arc::new(VideoMetadataExtractor::new()),
            hasher: Arc::new(Blake3MediaHasher::new()),
            counters: Arc::new(AtomicExtractionCounters::new()),
        }
    }

    /// Access the thread-safe atomic extraction counters.
    pub fn counters(&self) -> Arc<AtomicExtractionCounters> {
        Arc::clone(&self.counters)
    }

    /// Point-in-time extraction metrics snapshot.
    pub fn metrics(&self, start_time: Instant) -> ExtractionMetrics {
        self.counters.snapshot(start_time.elapsed())
    }

    /// Point-in-time extraction metrics snapshot given a custom elapsed duration.
    pub fn metrics_with_duration(&self, elapsed: Duration) -> ExtractionMetrics {
        self.counters.snapshot(elapsed)
    }

    /// Fallback document generation when an extractor fails with an unrecoverable error.
    fn make_failed_document(
        item: &DiscoveredMediaItem,
        scanned_root: Option<&Path>,
        err: &ExtractionError,
    ) -> MediaMetadataDocument {
        let root = scanned_root
            .map(|p| p.to_path_buf())
            .or_else(|| item.path.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| item.path.clone());

        let relative_path = item
            .path
            .strip_prefix(&root)
            .unwrap_or(&item.path)
            .to_path_buf();

        let file_name = item
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let extension = item
            .path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let file_context = FileContext {
            scanned_root: root,
            file_path: item.path.clone(),
            relative_path,
            file_name,
            file_size_bytes: item.file_size_bytes,
            extension,
            media_category: item.category,
        };

        let modified = std::fs::metadata(&item.path)
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::now());

        MediaMetadataDocument {
            id: Uuid::new_v4(),
            file_context,
            temporal: TemporalMetadata::from_system_time(modified),
            camera: None,
            exposure: None,
            location: None,
            dimensions: None,
            video: None,
            fingerprint: None,
            extraction_status: ExtractionStatus {
                status: ExtractionResultKind::Failed,
                warnings: Vec::new(),
                error_message: Some(err.to_string()),
            },
        }
    }
}

impl MediaMetadataExtractor for MetadataExtractionEngine {
    fn extract_file(
        &self,
        item: &DiscoveredMediaItem,
        scanned_root: Option<&Path>,
    ) -> Result<MediaMetadataDocument, ExtractionError> {
        let _span = tracing::debug_span!(
            "metadata_extract_file",
            path = %item.path.display(),
            category = ?item.category
        );
        let _enter = _span.enter();

        let res = match item.category {
            MediaCategory::Video => self.video_extractor.extract_file(item, scanned_root),
            MediaCategory::Photo | MediaCategory::RawPhoto => {
                self.exif_extractor.extract_file(item, scanned_root)
            }
        };

        let mut doc = match res {
            Ok(doc) => {
                tracing::debug!(
                    path = %item.path.display(),
                    status = ?doc.extraction_status.status,
                    "Metadata extracted successfully"
                );
                doc
            }
            Err(ref err) => {
                tracing::warn!(
                    path = %item.path.display(),
                    error = %err,
                    "Metadata extraction failed; generating fallback diagnostic document"
                );
                Self::make_failed_document(item, scanned_root, err)
            }
        };

        // Automatically compute cryptographic & perceptual content fingerprints
        let fp_config = FingerprintConfig::default();
        match self.hasher.fingerprint_file(item, Some(&doc), &fp_config) {
            Ok(fp) => {
                doc.fingerprint = Some(fp);
            }
            Err(err) => {
                tracing::warn!(
                    path = %item.path.display(),
                    error = %err,
                    "Content fingerprinting failed; recording diagnostic warning"
                );
                doc.extraction_status
                    .warnings
                    .push(format!("Fingerprint error: {}", err));
            }
        }

        self.counters.record(doc.extraction_status.status);
        Ok(doc)
    }
}

#[async_trait::async_trait]
impl BatchMetadataExtractor for MetadataExtractionEngine {
    async fn extract_batch(
        &self,
        batch: DiscoveryBatch,
        scanned_root: Option<&Path>,
    ) -> Vec<MediaMetadataDocument> {
        let root = scanned_root.map(|p| p.to_path_buf());
        let engine = self.clone();

        tokio::task::spawn_blocking(move || {
            batch
                .items
                .par_iter()
                .map(|item| {
                    engine
                        .extract_file(item, root.as_deref())
                        .unwrap_or_else(|err| {
                            MetadataExtractionEngine::make_failed_document(
                                item,
                                root.as_deref(),
                                &err,
                            )
                        })
                })
                .collect()
        })
        .await
        .unwrap_or_default()
    }
}
