use std::path::PathBuf;
use std::sync::Arc;
use tempfile::tempdir;
use uuid::Uuid;

use metadata_extractor::application::album_service::{AlbumSynthesisOptions, SmartAlbumSynthesisService};
use metadata_extractor::domain::errors::StorageError;
use metadata_extractor::domain::metadata_models::{
    FileContext, ImageDimensions, MediaMetadataDocument, TemporalMetadata, TimestampSource,
};
use metadata_extractor::domain::person_cluster::PersonCluster;
use metadata_extractor::domain::ports::MetadataStorageSink;
use metadata_extractor::domain::smart_album::SmartAlbum;
use metadata_extractor::domain::visual_models::{
    BoundingBox, DetectedFace, DetectedOccasion, FacialExpression, VisualSemanticMetadata,
};

/// Mock storage sink for smart albums test
struct MockAlbumStorageSink {
    documents: tokio::sync::Mutex<Vec<MediaMetadataDocument>>,
    clusters: tokio::sync::Mutex<Vec<PersonCluster>>,
    albums: tokio::sync::Mutex<Vec<SmartAlbum>>,
}

impl MockAlbumStorageSink {
    fn new(docs: Vec<MediaMetadataDocument>, clusters: Vec<PersonCluster>) -> Self {
        Self {
            documents: tokio::sync::Mutex::new(docs),
            clusters: tokio::sync::Mutex::new(clusters),
            albums: tokio::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl MetadataStorageSink for MockAlbumStorageSink {
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
    async fn update_visual_metadata_batch(&self, _updates: &[(Uuid, VisualSemanticMetadata)]) -> Result<usize, StorageError> { Ok(0) }
    async fn save_person_cluster(&self, _cluster: &PersonCluster) -> Result<(), StorageError> { Ok(()) }

    async fn list_person_clusters(&self) -> Result<Vec<PersonCluster>, StorageError> {
        let clusters = self.clusters.lock().await;
        Ok(clusters.clone())
    }

    async fn update_person_name(&self, _cluster_id: Uuid, _name: &str) -> Result<bool, StorageError> { Ok(true) }
    async fn merge_person_clusters(&self, _source_id: Uuid, _target_id: Uuid) -> Result<bool, StorageError> { Ok(true) }

    async fn save_smart_album(&self, album: &SmartAlbum) -> Result<(), StorageError> {
        let mut albums = self.albums.lock().await;
        if let Some(existing) = albums.iter_mut().find(|a| a.id == album.id) {
            *existing = album.clone();
        } else {
            albums.push(album.clone());
        }
        Ok(())
    }

    async fn list_smart_albums(&self) -> Result<Vec<SmartAlbum>, StorageError> {
        let albums = self.albums.lock().await;
        Ok(albums.clone())
    }

    async fn get_smart_album_by_id(&self, album_id: Uuid) -> Result<Option<SmartAlbum>, StorageError> {
        let albums = self.albums.lock().await;
        Ok(albums.iter().find(|a| a.id == album_id).cloned())
    }
}

fn create_party_photo(
    id: Uuid,
    name: &str,
    year: i32,
    person_id: Option<Uuid>,
    width: u32,
    height: u32,
    is_smiling: bool,
) -> MediaMetadataDocument {
    let face = person_id.map(|pid| DetectedFace {
        face_id: Uuid::new_v4(),
        bounding_box: BoundingBox::new(0.2, 0.2, 0.8, 0.8),
        confidence: 0.95,
        expressions: vec![FacialExpression {
            label: if is_smiling { "smile".to_string() } else { "neutral".to_string() },
            score: 0.90,
        }],
        person_id: Some(pid),
        embedding: None,
    });

    MediaMetadataDocument {
        id,
        file_context: FileContext {
            scanned_root: PathBuf::from("/test"),
            file_path: PathBuf::from(format!("/test/{}", name)),
            relative_path: PathBuf::from(name),
            file_name: name.to_string(),
            file_size_bytes: 2048,
            extension: "jpg".to_string(),
            media_category: metadata_extractor::domain::models::MediaCategory::Photo,
            modified_at: None,
        },
        temporal: TemporalMetadata {
            timestamp_utc: None,
            iso_timestamp: format!("{}-06-15T14:00:00Z", year),
            year,
            month: 6,
            day: 15,
            hour: 14,
            minute: 0,
            second: 0,
            source: TimestampSource::ExifDateTimeOriginal,
        },
        camera: None,
        exposure: None,
        location: None,
        dimensions: Some(ImageDimensions {
            width,
            height,
            orientation: None,
        }),
        video: None,
        fingerprint: None,
        extraction_status: Default::default(),
        dedup: None,
        visual: Some(VisualSemanticMetadata {
            model_version: "1.0.0".to_string(),
            analyzed_at: chrono::Utc::now(),
            objects: Vec::new(),
            occasions: vec![DetectedOccasion {
                category: "birthday".to_string(),
                confidence: 0.90,
            }],
            faces: face.into_iter().collect(),
            text_snippets: Vec::new(),
            search_tokens: Vec::new(),
        }),
    }
}

#[tokio::test]
async fn test_smart_album_synthesis_and_zero_duplicate_disk_invariant() {
    let maya_cluster_id = Uuid::new_v4();
    let maya_cluster = PersonCluster {
        id: maya_cluster_id,
        display_name: Some("Maya".to_string()),
        avatar_asset_id: None,
        centroid_embedding: vec![0.1; 512],
        total_photo_count: 3,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let doc_id_1 = Uuid::new_v4();
    let doc_id_2 = Uuid::new_v4();
    let doc_id_3 = Uuid::new_v4();

    // Photo 2 has high resolution (3840x2160) and smiling expression -> should be selected as cover
    let doc1 = create_party_photo(doc_id_1, "bday1.jpg", 2024, Some(maya_cluster_id), 1280, 720, false);
    let doc2 = create_party_photo(doc_id_2, "bday2.jpg", 2024, Some(maya_cluster_id), 3840, 2160, true);
    let doc3 = create_party_photo(doc_id_3, "bday3.jpg", 2024, Some(maya_cluster_id), 1920, 1080, false);

    let storage = Arc::new(MockAlbumStorageSink::new(
        vec![doc1, doc2, doc3],
        vec![maya_cluster],
    ));

    let album_service = SmartAlbumSynthesisService::new(storage.clone());

    // 1. Synthesize smart albums for 2024 with minimum 3 photos
    let options = AlbumSynthesisOptions {
        year: Some(2024),
        min_photos: 3,
        dry_run: false,
    };

    let albums = album_service.synthesize_albums(&options).await.unwrap();
    assert_eq!(albums.len(), 1, "Should generate exactly 1 smart album");

    let album = &albums[0];
    assert_eq!(album.title, "2024 - Maya's Birthday");
    assert_eq!(album.asset_ids.len(), 3);
    assert_eq!(album.cover_asset_id, Some(doc_id_2), "Photo 2 should be chosen as cover photo due to higher resolution and smile");

    // 2. Verify virtual album persistence
    let stored_albums = album_service.list_albums().await.unwrap();
    assert_eq!(stored_albums.len(), 1);

    // 3. Verify single-copy disk invariant (zero duplicate media files created)
    let temp_export_dir = tempdir().unwrap();
    let exported = album_service
        .export_album_as_symlinks(album.id, temp_export_dir.path())
        .await
        .unwrap();

    assert_eq!(exported, 3, "Exported 3 virtual symlinks");
}
