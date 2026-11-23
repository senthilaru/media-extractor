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

/// Mock Storage Sink for visual enrichment testing
struct MockVisualStorageSink {
    documents: tokio::sync::Mutex<Vec<MediaMetadataDocument>>,
}

impl MockVisualStorageSink {
    fn new(docs: Vec<MediaMetadataDocument>) -> Self {
        Self {
            documents: tokio::sync::Mutex::new(docs),
        }
    }
}

#[async_trait::async_trait]
impl MetadataStorageSink for MockVisualStorageSink {
    async fn ping(&self) -> Result<(), StorageError> {
        Ok(())
    }

    async fn ensure_indexes(&self) -> Result<(), StorageError> {
        Ok(())
    }

    async fn save_batch(
        &self,
        docs: &[MediaMetadataDocument],
    ) -> Result<metadata_extractor::domain::ports::BatchWriteStats, StorageError> {
        let mut storage = self.documents.lock().await;
        for doc in docs {
            storage.push(doc.clone());
        }
        Ok(metadata_extractor::domain::ports::BatchWriteStats {
            matched_count: docs.len() as u64,
            modified_count: 0,
            upserted_count: docs.len() as u64,
            failed_count: 0,
        })
    }

    async fn flush(&self) -> Result<(), StorageError> {
        Ok(())
    }

    async fn filter_unmodified_items(
        &self,
        _items: &[metadata_extractor::domain::models::DiscoveredMediaItem],
    ) -> Result<metadata_extractor::domain::metadata_models::FilterEvaluationResult, StorageError> {
        Ok(Default::default())
    }

    async fn query_duplicate_groups(
        &self,
    ) -> Result<Vec<(String, Vec<MediaMetadataDocument>)>, StorageError> {
        Ok(Vec::new())
    }

    async fn update_dedup_batch(
        &self,
        _updates: &[(Uuid, metadata_extractor::domain::dedup_models::DeduplicationMetadata)],
    ) -> Result<usize, StorageError> {
        Ok(0)
    }

    async fn query_removal_candidates(
        &self,
        _mount_prefix: Option<&str>,
    ) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        Ok(Vec::new())
    }

    async fn query_unmastered_candidates(
        &self,
        _mount_prefix: Option<&str>,
    ) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        Ok(Vec::new())
    }

    async fn find_document_by_id(
        &self,
        doc_id: Uuid,
    ) -> Result<Option<MediaMetadataDocument>, StorageError> {
        let storage = self.documents.lock().await;
        Ok(storage.iter().find(|d| d.id == doc_id).cloned())
    }

    async fn mark_removed(&self, _doc_id: Uuid) -> Result<(), StorageError> {
        Ok(())
    }

    async fn delete_document(&self, _doc_id: Uuid) -> Result<bool, StorageError> {
        Ok(true)
    }

    async fn delete_documents(&self, doc_ids: &[Uuid]) -> Result<usize, StorageError> {
        Ok(doc_ids.len())
    }

    async fn purge_removed_records(
        &self,
        _mount_prefix: Option<&str>,
    ) -> Result<usize, StorageError> {
        Ok(0)
    }

    async fn query_all_content_groups(
        &self,
        _mount_prefix: Option<&str>,
    ) -> Result<Vec<(String, Vec<MediaMetadataDocument>)>, StorageError> {
        Ok(Vec::new())
    }

    async fn query_canonical_masters(&self) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        let storage = self.documents.lock().await;
        Ok(storage.clone())
    }

    async fn query_replica_documents(
        &self,
        _replica_drive_id: &str,
    ) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        Ok(Vec::new())
    }

    async fn update_visual_metadata_batch(
        &self,
        updates: &[(Uuid, VisualSemanticMetadata)],
    ) -> Result<usize, StorageError> {
        let mut storage = self.documents.lock().await;
        let mut updated = 0;
        for (id, visual) in updates {
            if let Some(doc) = storage.iter_mut().find(|d| d.id == *id) {
                doc.visual = Some(visual.clone());
                updated += 1;
            }
        }
        Ok(updated)
    }
}

/// Mock Visual Inference Port
struct MockVisionPort;

#[async_trait::async_trait]
impl VisualInferencePort for MockVisionPort {
    async fn is_available(&self) -> bool {
        true
    }

    async fn infer_objects_and_scenes(
        &self,
        file_paths: &[PathBuf],
    ) -> Result<Vec<(PathBuf, Vec<DetectedObject>, Vec<DetectedOccasion>)>, ExtractionError> {
        let mut results = Vec::new();
        for p in file_paths {
            let fname = p.file_name().unwrap_or_default().to_string_lossy();
            let mut objs = Vec::new();
            let mut occs = Vec::new();

            if fname.contains("teddy") {
                objs.push(DetectedObject {
                    label: "teddy_bear".to_string(),
                    confidence: 0.95,
                    bounding_box: Some(BoundingBox::new(0.1, 0.1, 0.8, 0.8)),
                });
            }
            if fname.contains("cycle") {
                objs.push(DetectedObject {
                    label: "cycle".to_string(),
                    confidence: 0.92,
                    bounding_box: Some(BoundingBox::new(0.2, 0.2, 0.7, 0.7)),
                });
            }
            if fname.contains("birthday") {
                occs.push(DetectedOccasion {
                    category: "birthday".to_string(),
                    confidence: 0.88,
                });
            }

            results.push((p.clone(), objs, occs));
        }
        Ok(results)
    }

    async fn infer_faces(
        &self,
        _file_paths: &[PathBuf],
    ) -> Result<Vec<(PathBuf, Vec<DetectedFace>)>, ExtractionError> {
        Ok(Vec::new())
    }

    async fn infer_ocr(
        &self,
        _file_paths: &[PathBuf],
    ) -> Result<Vec<(PathBuf, Vec<ExtractedTextSnippet>, Vec<String>)>, ExtractionError> {
        Ok(Vec::new())
    }
}

fn create_dummy_jpeg(path: &Path) {
    let mut file = File::create(path).unwrap();
    // Minimal valid JPEG header bytes + dummy payload
    file.write_all(&[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, 0x4A, 0x46, 0x49, 0x46, 0x00, 0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xD9]).unwrap();
}

#[tokio::test]
async fn test_visual_enrichment_preserves_file_immutability() {
    let dir = tempdir().unwrap();
    let img_path = dir.path().join("teddy_bear_birthday.jpg");
    create_dummy_jpeg(&img_path);

    // 1. Verify read-only guard can open the file without write permissions
    let guard_res = ReadOnlyFileGuard::verify_read_only(&img_path);
    assert!(guard_res.is_ok(), "ReadOnlyFileGuard should succeed on valid file");

    // 2. Compute BLAKE3 cryptographic hash before enrichment
    let initial_hash = blake3::hash(&std::fs::read(&img_path).unwrap()).to_hex().to_string();

    // 3. Create document in mock storage
    let doc_id = Uuid::new_v4();
    let doc = MediaMetadataDocument {
        id: doc_id,
        file_context: FileContext {
            scanned_root: dir.path().to_path_buf(),
            file_path: img_path.clone(),
            relative_path: PathBuf::from("teddy_bear_birthday.jpg"),
            file_name: "teddy_bear_birthday.jpg".to_string(),
            file_size_bytes: 22,
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

    let storage = Arc::new(MockVisualStorageSink::new(vec![doc]));
    let vision = Arc::new(MockVisionPort);
    let enrichment_service = VisualEnrichmentService::new(storage.clone(), vision);

    // 4. Run enrichment
    let options = EnrichOptions {
        source: Some(dir.path().to_path_buf()),
        enable_objects: true,
        enable_faces: false,
        enable_ocr: false,
        min_confidence: 0.60,
        batch_size: 10,
        force: true,
        dry_run: false,
    };

    let report = enrichment_service.execute_enrichment(&options).await.unwrap();
    assert_eq!(report.total_enriched, 1);
    assert_eq!(report.objects_detected, 1);
    assert_eq!(report.occasions_detected, 1);

    // 5. Verify the file on disk was NOT modified (bit-for-bit immutable, SC-006)
    let post_hash = blake3::hash(&std::fs::read(&img_path).unwrap()).to_hex().to_string();
    assert_eq!(
        initial_hash, post_hash,
        "BLAKE3 hash must remain identical before and after enrichment (SC-006)"
    );

    // 6. Verify stored visual metadata
    let enriched_doc = storage.find_document_by_id(doc_id).await.unwrap().unwrap();
    let visual = enriched_doc.visual.expect("Visual metadata should be populated");
    assert_eq!(visual.objects.len(), 1);
    assert_eq!(visual.objects[0].label, "teddy_bear");
    assert!(visual.objects[0].confidence >= 0.60);
    assert_eq!(visual.occasions.len(), 1);
    assert_eq!(visual.occasions[0].category, "birthday");

    // 7. Verify search service queries
    let search_service = SearchService::new(storage);

    // Search by object
    let obj_search = search_service
        .execute_search(&SearchCriteria {
            object: Some("teddy_bear".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(obj_search.len(), 1);
    assert_eq!(obj_search[0].id, doc_id);

    // Search by occasion
    let occ_search = search_service
        .execute_search(&SearchCriteria {
            occasion: Some("birthday".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(occ_search.len(), 1);
    assert_eq!(occ_search[0].id, doc_id);

    // Negative search (cycle not in this document)
    let neg_search = search_service
        .execute_search(&SearchCriteria {
            object: Some("cycle".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(neg_search.len(), 0);
}
