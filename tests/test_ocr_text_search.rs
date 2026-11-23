use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tempfile::tempdir;
use uuid::Uuid;

use metadata_extractor::application::search_service::{SearchCriteria, SearchService};
use metadata_extractor::application::visual_service::{EnrichOptions, VisualEnrichmentService};
use metadata_extractor::domain::errors::{ExtractionError, StorageError};
use metadata_extractor::domain::metadata_models::{
    FileContext, MediaMetadataDocument, TemporalMetadata, TimestampSource,
};
use metadata_extractor::domain::ports::{MetadataStorageSink, VisualInferencePort};
use metadata_extractor::domain::visual_models::{
    BoundingBox, DetectedFace, DetectedObject, DetectedOccasion, ExtractedTextSnippet,
    VisualSemanticMetadata,
};
use metadata_extractor::infrastructure::vision_adapter::ReadOnlyFileGuard;

/// In-memory mock storage sink for OCR tests
struct MockOcrStorageSink {
    documents: tokio::sync::Mutex<Vec<MediaMetadataDocument>>,
}

impl MockOcrStorageSink {
    fn new(docs: Vec<MediaMetadataDocument>) -> Self {
        Self {
            documents: tokio::sync::Mutex::new(docs),
        }
    }
}

#[async_trait::async_trait]
impl MetadataStorageSink for MockOcrStorageSink {
    async fn ping(&self) -> Result<(), StorageError> { Ok(()) }
    async fn ensure_indexes(&self) -> Result<(), StorageError> { Ok(()) }
    async fn save_batch(&self, _docs: &[MediaMetadataDocument]) -> Result<metadata_extractor::domain::ports::BatchWriteStats, StorageError> { Ok(Default::default()) }
    async fn flush(&self) -> Result<(), StorageError> { Ok(()) }
    async fn filter_unmodified_items(&self, _items: &[metadata_extractor::domain::models::DiscoveredMediaItem]) -> Result<metadata_extractor::domain::metadata_models::FilterEvaluationResult, StorageError> { Ok(Default::default()) }
    async fn query_duplicate_groups(&self) -> Result<Vec<(String, Vec<MediaMetadataDocument>)>, StorageError> { Ok(Vec::new()) }
    async fn update_dedup_batch(&self, _updates: &[(Uuid, metadata_extractor::domain::dedup_models::DeduplicationMetadata)]) -> Result<usize, StorageError> { Ok(0) }
    async fn query_removal_candidates(&self, _mount_prefix: Option<&str>) -> Result<Vec<MediaMetadataDocument>, StorageError> { Ok(Vec::new()) }
    async fn query_unmastered_candidates(&self, _mount_prefix: Option<&str>) -> Result<Vec<MediaMetadataDocument>, StorageError> { Ok(Vec::new()) }
    async fn find_document_by_id(&self, doc_id: Uuid) -> Result<Option<MediaMetadataDocument>, StorageError> {
        let storage = self.documents.lock().await;
        Ok(storage.iter().find(|d| d.id == doc_id).cloned())
    }
    async fn mark_removed(&self, _doc_id: Uuid) -> Result<(), StorageError> { Ok(()) }
    async fn delete_document(&self, _doc_id: Uuid) -> Result<bool, StorageError> { Ok(true) }
    async fn delete_documents(&self, doc_ids: &[Uuid]) -> Result<usize, StorageError> { Ok(doc_ids.len()) }
    async fn purge_removed_records(&self, _mount_prefix: Option<&str>) -> Result<usize, StorageError> { Ok(0) }
    async fn query_all_content_groups(&self, _mount_prefix: Option<&str>) -> Result<Vec<(String, Vec<MediaMetadataDocument>)>, StorageError> { Ok(Vec::new()) }
    
    async fn query_canonical_masters(&self) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        let storage = self.documents.lock().await;
        Ok(storage.clone())
    }

    async fn query_replica_documents(&self, _replica_drive_id: &str) -> Result<Vec<MediaMetadataDocument>, StorageError> { Ok(Vec::new()) }

    async fn update_visual_metadata_batch(
        &self,
        updates: &[(Uuid, VisualSemanticMetadata)],
    ) -> Result<usize, StorageError> {
        let mut storage = self.documents.lock().await;
        let mut count = 0;
        for (id, visual) in updates {
            if let Some(doc) = storage.iter_mut().find(|d| d.id == *id) {
                doc.visual = Some(visual.clone());
                count += 1;
            }
        }
        Ok(count)
    }
}

/// Mock OCR Inference Port
struct MockOcrVisionPort;

#[async_trait::async_trait]
impl VisualInferencePort for MockOcrVisionPort {
    async fn is_available(&self) -> bool { true }

    async fn infer_objects_and_scenes(
        &self,
        _file_paths: &[PathBuf],
    ) -> Result<Vec<(PathBuf, Vec<DetectedObject>, Vec<DetectedOccasion>)>, ExtractionError> {
        Ok(Vec::new())
    }

    async fn infer_faces(
        &self,
        _file_paths: &[PathBuf],
    ) -> Result<Vec<(PathBuf, Vec<DetectedFace>)>, ExtractionError> {
        Ok(Vec::new())
    }

    async fn infer_ocr(
        &self,
        file_paths: &[PathBuf],
    ) -> Result<Vec<(PathBuf, Vec<ExtractedTextSnippet>, Vec<String>)>, ExtractionError> {
        let mut results = Vec::new();
        for p in file_paths {
            let fname = p.file_name().unwrap_or_default().to_string_lossy();
            let mut snippets = Vec::new();
            let mut tokens = Vec::new();

            if fname.contains("liam") {
                snippets.push(ExtractedTextSnippet {
                    text: "Happy 5th Birthday Liam".to_string(),
                    tokens: vec!["happy".into(), "5th".into(), "birthday".into(), "liam".into()],
                    confidence: 0.96,
                    bounding_box: Some(BoundingBox::new(0.1, 0.1, 0.9, 0.3)),
                });
                tokens.extend(vec!["happy".into(), "5th".into(), "birthday".into(), "liam".into()]);
            }

            results.push((p.clone(), snippets, tokens));
        }
        Ok(results)
    }
}

fn create_dummy_file(path: &Path) {
    let mut file = File::create(path).unwrap();
    file.write_all(b"dummy image bytes for test").unwrap();
}

#[tokio::test]
async fn test_ocr_extraction_and_in_image_text_search() {
    let dir = tempdir().unwrap();
    let img_path = dir.path().join("party_liam_banner.jpg");
    create_dummy_file(&img_path);

    assert!(ReadOnlyFileGuard::verify_read_only(&img_path).is_ok());

    let doc_id = Uuid::new_v4();
    let doc = MediaMetadataDocument {
        id: doc_id,
        file_context: FileContext {
            scanned_root: dir.path().to_path_buf(),
            file_path: img_path.clone(),
            relative_path: PathBuf::from("party_liam_banner.jpg"),
            file_name: "party_liam_banner.jpg".to_string(),
            file_size_bytes: 26,
            extension: "jpg".to_string(),
            media_category: metadata_extractor::domain::models::MediaCategory::Photo,
            modified_at: None,
        },
        temporal: TemporalMetadata::from_utc_datetime(
            chrono::Utc::now(),
            TimestampSource::ExifDateTimeOriginal,
        ),
        camera: None,
        exposure: None,
        location: None,
        dimensions: None,
        video: None,
        fingerprint: None,
        extraction_status: Default::default(),
        dedup: None,
        visual: None,
    };

    let storage = Arc::new(MockOcrStorageSink::new(vec![doc]));
    let vision = Arc::new(MockOcrVisionPort);
    let enrichment_service = VisualEnrichmentService::new(storage.clone(), vision);

    // 1. Run enrichment with OCR enabled
    let options = EnrichOptions {
        source: Some(dir.path().to_path_buf()),
        enable_objects: false,
        enable_faces: false,
        enable_ocr: true,
        min_confidence: 0.60,
        batch_size: 10,
        force: true,
        dry_run: false,
    };

    let report = enrichment_service.execute_enrichment(&options).await.unwrap();
    assert_eq!(report.total_enriched, 1);
    assert_eq!(report.ocr_snippets_extracted, 1);

    // 2. Query search by word "Liam"
    let search_service = SearchService::new(storage.clone());
    let liam_results = search_service
        .execute_search(&SearchCriteria {
            text: Some("Liam".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(liam_results.len(), 1, "Should find the image containing 'Liam'");
    assert_eq!(liam_results[0].id, doc_id);

    // 3. Query search by word "Birthday"
    let bday_results = search_service
        .execute_search(&SearchCriteria {
            text: Some("Birthday".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(bday_results.len(), 1, "Should find the image containing 'Birthday'");

    // 4. Query search by non-existent word "Graduation"
    let negative_results = search_service
        .execute_search(&SearchCriteria {
            text: Some("Graduation".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(negative_results.len(), 0, "Should NOT find 'Graduation'");
}
