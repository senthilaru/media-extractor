use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

use metadata_extractor::application::face_service::FaceManagementService;
use metadata_extractor::application::search_service::{SearchCriteria, SearchService};
use metadata_extractor::domain::errors::StorageError;
use metadata_extractor::domain::metadata_models::{
    FileContext, MediaMetadataDocument, TemporalMetadata, TimestampSource,
};
use metadata_extractor::domain::person_cluster::PersonCluster;
use metadata_extractor::domain::ports::MetadataStorageSink;
use metadata_extractor::domain::visual_models::{
    BoundingBox, DetectedFace, FacialExpression, VisualSemanticMetadata,
};

/// In-memory mock storage sink for face clustering tests
struct MockFaceStorageSink {
    documents: tokio::sync::Mutex<Vec<MediaMetadataDocument>>,
    clusters: tokio::sync::Mutex<Vec<PersonCluster>>,
}

impl MockFaceStorageSink {
    fn new(docs: Vec<MediaMetadataDocument>) -> Self {
        Self {
            documents: tokio::sync::Mutex::new(docs),
            clusters: tokio::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl MetadataStorageSink for MockFaceStorageSink {
    async fn ping(&self) -> Result<(), StorageError> { Ok(()) }
    async fn ensure_indexes(&self) -> Result<(), StorageError> { Ok(()) }
    async fn save_batch(&self, _docs: &[MediaMetadataDocument]) -> Result<metadata_extractor::domain::ports::BatchWriteStats, StorageError> {
        Ok(Default::default())
    }
    async fn flush(&self) -> Result<(), StorageError> { Ok(()) }
    async fn filter_unmodified_items(&self, _items: &[metadata_extractor::domain::models::DiscoveredMediaItem]) -> Result<metadata_extractor::domain::metadata_models::FilterEvaluationResult, StorageError> {
        Ok(Default::default())
    }
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

    async fn save_person_cluster(&self, cluster: &PersonCluster) -> Result<(), StorageError> {
        let mut clusters = self.clusters.lock().await;
        if let Some(existing) = clusters.iter_mut().find(|c| c.id == cluster.id) {
            *existing = cluster.clone();
        } else {
            clusters.push(cluster.clone());
        }
        Ok(())
    }

    async fn list_person_clusters(&self) -> Result<Vec<PersonCluster>, StorageError> {
        let clusters = self.clusters.lock().await;
        Ok(clusters.clone())
    }

    async fn update_person_name(&self, cluster_id: Uuid, name: &str) -> Result<bool, StorageError> {
        let mut clusters = self.clusters.lock().await;
        if let Some(c) = clusters.iter_mut().find(|c| c.id == cluster_id) {
            c.display_name = Some(name.to_string());
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn merge_person_clusters(&self, source_id: Uuid, target_id: Uuid) -> Result<bool, StorageError> {
        let mut clusters = self.clusters.lock().await;
        let source_idx = clusters.iter().position(|c| c.id == source_id);
        let target_idx = clusters.iter().position(|c| c.id == target_id);

        if let (Some(s_idx), Some(t_idx)) = (source_idx, target_idx) {
            let source = clusters.remove(s_idx);
            let target_adj = if s_idx < t_idx { t_idx - 1 } else { t_idx };
            clusters[target_adj].merge_with(&source);

            // Update documents face person_ids
            let mut docs = self.documents.lock().await;
            for d in docs.iter_mut() {
                if let Some(ref mut vis) = d.visual {
                    for f in &mut vis.faces {
                        if f.person_id == Some(source_id) {
                            f.person_id = Some(target_id);
                        }
                    }
                }
            }
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

fn create_sample_doc(id: Uuid, name: &str, embedding: Vec<f32>, expr: &str) -> MediaMetadataDocument {
    let face = DetectedFace {
        face_id: Uuid::new_v4(),
        bounding_box: BoundingBox::new(0.2, 0.2, 0.6, 0.6),
        confidence: 0.95,
        expressions: vec![FacialExpression {
            label: expr.to_string(),
            score: 0.91,
        }],
        person_id: None,
        embedding: Some(embedding),
    };

    MediaMetadataDocument {
        id,
        file_context: FileContext {
            scanned_root: PathBuf::from("/test"),
            file_path: PathBuf::from(format!("/test/{}", name)),
            relative_path: PathBuf::from(name),
            file_name: name.to_string(),
            file_size_bytes: 1024,
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
        visual: Some(VisualSemanticMetadata {
            model_version: "1.0.0".to_string(),
            analyzed_at: chrono::Utc::now(),
            objects: Vec::new(),
            occasions: Vec::new(),
            faces: vec![face],
            text_snippets: Vec::new(),
            search_tokens: Vec::new(),
        }),
    }
}

#[tokio::test]
async fn test_face_detection_clustering_naming_and_search() {
    // 1. Create two embeddings very close to each other (Maya)
    let mut maya_emb_1 = vec![0.0f32; 512];
    maya_emb_1[0] = 1.0;
    let mut maya_emb_2 = vec![0.0f32; 512];
    maya_emb_2[0] = 0.98;
    maya_emb_2[1] = 0.05;

    // And one different embedding (Grandpa)
    let mut grandpa_emb = vec![0.0f32; 512];
    grandpa_emb[50] = 1.0;

    let doc_id_1 = Uuid::new_v4();
    let doc_id_2 = Uuid::new_v4();
    let doc_id_3 = Uuid::new_v4();

    let doc1 = create_sample_doc(doc_id_1, "maya_smiling.jpg", maya_emb_1, "smile");
    let doc2 = create_sample_doc(doc_id_2, "maya_happy.jpg", maya_emb_2, "happy");
    let doc3 = create_sample_doc(doc_id_3, "grandpa.jpg", grandpa_emb, "neutral");

    let storage = Arc::new(MockFaceStorageSink::new(vec![doc1, doc2, doc3]));
    let face_service = FaceManagementService::new(storage.clone());

    // 2. Perform clustering
    let new_clusters = face_service.cluster_faces().await.unwrap();
    assert_eq!(new_clusters, 2, "Should establish exactly 2 identity clusters (Maya & Grandpa)");

    let clusters = face_service.list_clusters(false, None).await.unwrap();
    assert_eq!(clusters.len(), 2);

    // Maya's cluster should contain 2 photos
    let maya_cluster = clusters.iter().find(|c| c.total_photo_count == 2).unwrap();
    let grandpa_cluster = clusters.iter().find(|c| c.total_photo_count == 1).unwrap();

    // 3. Assign display name to Maya's cluster
    let name_success = face_service.name_cluster(maya_cluster.id, "Maya").await.unwrap();
    assert!(name_success);

    let name_success_2 = face_service.name_cluster(grandpa_cluster.id, "Grandpa").await.unwrap();
    assert!(name_success_2);

    // 3b. Verify get_cluster_photos for inspecting top matching files
    let maya_details = face_service.get_cluster_photos(maya_cluster.id, Some(10)).await.unwrap().unwrap();
    assert_eq!(maya_details.total_photos, 2);
    assert_eq!(maya_details.top_photos.len(), 2);
    assert_eq!(maya_details.top_photos[0].doc_id, doc_id_1);
    assert_eq!(maya_details.top_photos[0].file_path, PathBuf::from("/test/maya_smiling.jpg"));

    // 4. Test multi-modal search by person name, cluster_id, and facial expression
    let search_service = SearchService::new(storage.clone());

    // Search by cluster_id directly
    let cluster_results = search_service
        .execute_search(&SearchCriteria {
            cluster_id: Some(maya_cluster.id),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(cluster_results.len(), 2, "Searching by cluster_id should return both of Maya's photos");

    // Search for Maya with smile
    let maya_smile = search_service
        .execute_search(&SearchCriteria {
            person: Some("Maya".to_string()),
            expression: Some("smile".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(maya_smile.len(), 1);
    assert_eq!(maya_smile[0].id, doc_id_1);

    // Search for Maya with happy
    let maya_happy = search_service
        .execute_search(&SearchCriteria {
            person: Some("Maya".to_string()),
            expression: Some("happy".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(maya_happy.len(), 1);
    assert_eq!(maya_happy[0].id, doc_id_2);

    // Search for Grandpa
    let grandpa_search = search_service
        .execute_search(&SearchCriteria {
            person: Some("Grandpa".to_string()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(grandpa_search.len(), 1);
    assert_eq!(grandpa_search[0].id, doc_id_3);

    // 5. Test merging two clusters
    let merge_success = face_service.merge_clusters(grandpa_cluster.id, maya_cluster.id).await.unwrap();
    assert!(merge_success);

    let updated_clusters = face_service.list_clusters(false, None).await.unwrap();
    assert_eq!(updated_clusters.len(), 1, "After merge, exactly 1 unified cluster remains");
    assert_eq!(updated_clusters[0].total_photo_count, 3);
}
