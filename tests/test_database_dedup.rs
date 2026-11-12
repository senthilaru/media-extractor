use chrono::Utc;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use metadata_extractor::application::dedup_engine::MediaDeduplicationEngine;
use metadata_extractor::domain::dedup_models::{
    DeduplicationMetadata, DeduplicationStatus, DriveConfiguration, DriveDefinition,
    DriveGroupSpec, DriveRules, MediaFingerprint,
};
use metadata_extractor::domain::errors::StorageError;
use metadata_extractor::domain::metadata_models::{
    ExtractionStatus, FileContext, FilterEvaluationResult, MediaMetadataDocument, TemporalMetadata,
    TimestampSource,
};
use metadata_extractor::domain::models::{DiscoveredMediaItem, MediaCategory};
use metadata_extractor::domain::ports::{BatchWriteStats, MetadataStorageSink};
use metadata_extractor::infrastructure::drives_config::CanonicalPathMatcher;

fn create_test_config() -> DriveConfiguration {
    let mut groups = HashMap::new();
    groups.insert(
        "primary".to_string(),
        DriveGroupSpec {
            priority: 100,
            description: Some("Primary Master".to_string()),
        },
    );
    groups.insert(
        "replica1".to_string(),
        DriveGroupSpec {
            priority: 80,
            description: Some("First Mirror Replica".to_string()),
        },
    );

    let drives = vec![
        DriveDefinition {
            id: "primary_main".to_string(),
            group: "primary".to_string(),
            mount_path: PathBuf::from("/Volumes/SP-1"),
            master_path: PathBuf::from("/Volumes/SP-1/SPSA/media/master"),
        },
        DriveDefinition {
            id: "replica_1".to_string(),
            group: "replica1".to_string(),
            mount_path: PathBuf::from("/Volumes/SP-2"),
            master_path: PathBuf::from("/Volumes/SP-2/SPSA/media/master"),
        },
    ];

    DriveConfiguration {
        version: "1.0".to_string(),
        groups,
        drives,
        rules: DriveRules::default(),
    }
}

fn create_mock_doc(path_str: &str, size_bytes: u64, hash: &str) -> MediaMetadataDocument {
    let path = PathBuf::from(path_str);
    let now = Utc::now();
    let filename = path.file_name().unwrap().to_string_lossy().to_string();

    MediaMetadataDocument {
        id: Uuid::new_v4(),
        file_context: FileContext {
            scanned_root: path.components().take(2).collect::<PathBuf>(),
            file_path: path.clone(),
            relative_path: path.clone(),
            file_name: filename,
            file_size_bytes: size_bytes,
            extension: "jpg".to_string(),
            media_category: MediaCategory::Photo,
            modified_at: Some(now),
        },
        temporal: TemporalMetadata::from_utc_datetime(now, TimestampSource::ExifDateTimeOriginal),
        camera: None,
        exposure: None,
        location: None,
        dimensions: None,
        video: None,
        fingerprint: Some(MediaFingerprint {
            id: Uuid::new_v4(),
            file_path: path,
            file_size_bytes: size_bytes,
            media_category: MediaCategory::Photo,
            dimensions: None,
            sample_digest: None,
            cryptographic_hash: hash.to_string(),
            perceptual_hash: None,
            phash_hex: None,
            rotated_hashes: None,
            is_low_entropy: false,
            fingerprinted_at: now,
        }),
        extraction_status: ExtractionStatus::default(),
        dedup: None,
    }
}

type DuplicateGroupsList = Vec<(String, Vec<MediaMetadataDocument>)>;

#[derive(Default, Clone)]
struct MockStorageSink {
    duplicate_groups: Arc<Mutex<DuplicateGroupsList>>,
    received_updates: Arc<Mutex<Vec<(Uuid, DeduplicationMetadata)>>>,
}

#[async_trait::async_trait]
impl MetadataStorageSink for MockStorageSink {
    async fn ping(&self) -> Result<(), StorageError> {
        Ok(())
    }

    async fn ensure_indexes(&self) -> Result<(), StorageError> {
        Ok(())
    }

    async fn save_batch(
        &self,
        docs: &[MediaMetadataDocument],
    ) -> Result<BatchWriteStats, StorageError> {
        Ok(BatchWriteStats {
            matched_count: 0,
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
        _items: &[DiscoveredMediaItem],
    ) -> Result<FilterEvaluationResult, StorageError> {
        Ok(FilterEvaluationResult::default())
    }

    async fn query_duplicate_groups(
        &self,
    ) -> Result<Vec<(String, Vec<MediaMetadataDocument>)>, StorageError> {
        Ok(self.duplicate_groups.lock().unwrap().clone())
    }

    async fn update_dedup_batch(
        &self,
        updates: &[(Uuid, DeduplicationMetadata)],
    ) -> Result<usize, StorageError> {
        let mut guard = self.received_updates.lock().unwrap();
        guard.extend_from_slice(updates);
        Ok(updates.len())
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
        _doc_id: Uuid,
    ) -> Result<Option<MediaMetadataDocument>, StorageError> {
        Ok(None)
    }

    async fn mark_removed(&self, _doc_id: Uuid) -> Result<(), StorageError> {
        Ok(())
    }

    async fn query_all_content_groups(
        &self,
        _mount_prefix: Option<&str>,
    ) -> Result<Vec<(String, Vec<MediaMetadataDocument>)>, StorageError> {
        let guard = self.duplicate_groups.lock().unwrap();
        Ok(guard.clone())
    }

    async fn query_canonical_masters(&self) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        let guard = self.duplicate_groups.lock().unwrap();
        let mut masters = Vec::new();
        for (_, docs) in guard.iter() {
            for d in docs {
                if d.dedup
                    .as_ref()
                    .map(|m| m.status == DeduplicationStatus::CanonicalMaster)
                    .unwrap_or(false)
                {
                    masters.push(d.clone());
                }
            }
        }
        Ok(masters)
    }

    async fn query_replica_documents(
        &self,
        replica_drive_id: &str,
    ) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        let guard = self.duplicate_groups.lock().unwrap();
        let mut replicas = Vec::new();
        for (_, docs) in guard.iter() {
            for d in docs {
                if d.dedup
                    .as_ref()
                    .map(|m| {
                        m.status == DeduplicationStatus::ReplicaKeep
                            && m.drive_id.as_deref() == Some(replica_drive_id)
                    })
                    .unwrap_or(false)
                {
                    replicas.push(d.clone());
                }
            }
        }
        Ok(replicas)
    }
}

#[test]
fn test_evaluate_replica_cluster_master_and_replicas() {
    let config = create_test_config();
    let matcher = CanonicalPathMatcher::new(config);

    let doc_primary_master = create_mock_doc(
        "/Volumes/SP-1/SPSA/media/master/2021/30-Nov-2021/Photos/IMG_001.JPG",
        20_000_000,
        "hash_abc_123",
    );
    let doc_replica_master = create_mock_doc(
        "/Volumes/SP-2/SPSA/media/master/2021/30-Nov-2021/Photos/IMG_001.JPG",
        20_000_000,
        "hash_abc_123",
    );
    let doc_primary_scattered = create_mock_doc(
        "/Volumes/SP-1/dump/messy_folder/IMG_001.JPG",
        20_000_000,
        "hash_abc_123",
    );
    let doc_replica_scattered = create_mock_doc(
        "/Volumes/SP-2/downloads/IMG_001.JPG",
        20_000_000,
        "hash_abc_123",
    );

    let cluster = vec![
        doc_primary_master.clone(),
        doc_replica_master.clone(),
        doc_primary_scattered.clone(),
        doc_replica_scattered.clone(),
    ];

    let results = MediaDeduplicationEngine::evaluate_replica_cluster(&cluster, &matcher);
    assert_eq!(results.len(), 4);

    let res_map: HashMap<Uuid, DeduplicationMetadata> = results.into_iter().collect();

    // 1. Primary master check
    let primary_meta = res_map.get(&doc_primary_master.id).unwrap();
    assert_eq!(primary_meta.status, DeduplicationStatus::CanonicalMaster);
    assert_eq!(
        primary_meta.canonical_master_id,
        Some(doc_primary_master.id)
    );
    assert_eq!(
        primary_meta.canonical_master_path,
        Some(doc_primary_master.file_context.file_path.clone())
    );
    assert!(primary_meta.is_in_master_path);
    assert_eq!(primary_meta.reclaimable_bytes, 0);

    // 2. Replica keep check
    let replica_meta = res_map.get(&doc_replica_master.id).unwrap();
    assert_eq!(replica_meta.status, DeduplicationStatus::ReplicaKeep);
    assert_eq!(
        replica_meta.canonical_master_id,
        Some(doc_primary_master.id)
    );
    assert_eq!(
        replica_meta.canonical_master_path,
        Some(doc_primary_master.file_context.file_path.clone())
    );
    assert!(replica_meta.is_in_master_path);
    assert_eq!(replica_meta.reclaimable_bytes, 0);

    // 3. Primary scattered check -> MarkForRemoval
    let p_scattered_meta = res_map.get(&doc_primary_scattered.id).unwrap();
    assert_eq!(p_scattered_meta.status, DeduplicationStatus::MarkForRemoval);
    assert_eq!(
        p_scattered_meta.canonical_master_id,
        Some(doc_primary_master.id)
    );
    assert!(!p_scattered_meta.is_in_master_path);
    assert_eq!(p_scattered_meta.reclaimable_bytes, 20_000_000);

    // 4. Replica scattered check -> MarkForRemoval
    let r_scattered_meta = res_map.get(&doc_replica_scattered.id).unwrap();
    assert_eq!(r_scattered_meta.status, DeduplicationStatus::MarkForRemoval);
    assert_eq!(
        r_scattered_meta.canonical_master_id,
        Some(doc_primary_master.id)
    );
    assert!(!r_scattered_meta.is_in_master_path);
    assert_eq!(r_scattered_meta.reclaimable_bytes, 20_000_000);
}

#[test]
fn test_evaluate_replica_cluster_unique_outside_master() {
    let config = create_test_config();
    let matcher = CanonicalPathMatcher::new(config);

    // Duplicate pair on secondary/scattered folders, zero copies in master
    let doc_unmastered_1 = create_mock_doc(
        "/Volumes/SP-1/camera_dump/DSC0042.JPG",
        15_000_000,
        "hash_unique_unmastered",
    );
    let doc_unmastered_2 = create_mock_doc(
        "/Volumes/SP-2/backup_raw/DSC0042.JPG",
        15_000_000,
        "hash_unique_unmastered",
    );

    let cluster = vec![doc_unmastered_1.clone(), doc_unmastered_2.clone()];
    let results = MediaDeduplicationEngine::evaluate_content_group(&cluster, &matcher);

    assert_eq!(results.len(), 2);
    let res_map: HashMap<Uuid, DeduplicationMetadata> = results.into_iter().collect();

    // SP-1 has higher priority (100) than SP-2 (80), so doc 1 is selected for ingest
    let meta_1 = res_map.get(&doc_unmastered_1.id).unwrap();
    assert_eq!(meta_1.status, DeduplicationStatus::NeedsIngestToMaster);
    assert_eq!(meta_1.reclaimable_bytes, 0);

    // doc 2 is marked for removal to prevent duplicate ingestion
    let meta_2 = res_map.get(&doc_unmastered_2.id).unwrap();
    assert_eq!(meta_2.status, DeduplicationStatus::MarkForRemoval);
    assert_eq!(meta_2.reclaimable_bytes, 15_000_000);
}

#[test]
fn test_evaluate_content_group_single_unique_asset_outside_and_inside_master() {
    let config = create_test_config();
    let matcher = CanonicalPathMatcher::new(config);

    // 1. Single unique asset outside master (AC-1)
    let doc_unique_outside = create_mock_doc(
        "/Volumes/SP-2/staging/unique_photo.jpg",
        5_000_000,
        "hash_single_unique",
    );
    let results_outside = MediaDeduplicationEngine::evaluate_content_group(
        std::slice::from_ref(&doc_unique_outside),
        &matcher,
    );
    assert_eq!(results_outside.len(), 1);
    assert_eq!(
        results_outside[0].1.status,
        DeduplicationStatus::NeedsIngestToMaster
    );
    assert_eq!(results_outside[0].1.reclaimable_bytes, 0);

    // 2. Single unique asset inside canonical master on primary drive (AC-3)
    let doc_unique_master = create_mock_doc(
        "/Volumes/SP-1/SPSA/media/master/2024/15-Aug-2024/Photos/unique_photo.jpg",
        5_000_000,
        "hash_single_master",
    );
    let results_inside = MediaDeduplicationEngine::evaluate_content_group(
        std::slice::from_ref(&doc_unique_master),
        &matcher,
    );
    assert_eq!(results_inside.len(), 1);
    assert_eq!(
        results_inside[0].1.status,
        DeduplicationStatus::CanonicalMaster
    );
    assert_eq!(results_inside[0].1.reclaimable_bytes, 0);
}

#[tokio::test]
async fn test_run_universal_classification() {
    let config = create_test_config();
    let engine = MediaDeduplicationEngine::new();

    let doc_unique = create_mock_doc("/Volumes/SP-2/staging/unique.jpg", 3_000_000, "hash_unique");
    let doc_dup1 = create_mock_doc("/Volumes/SP-1/camera/dup.jpg", 4_000_000, "hash_dup");
    let doc_dup2 = create_mock_doc("/Volumes/SP-2/camera/dup.jpg", 4_000_000, "hash_dup");

    let content_groups = vec![
        ("hash_unique".to_string(), vec![doc_unique]),
        ("hash_dup".to_string(), vec![doc_dup1, doc_dup2]),
    ];

    let sink = MockStorageSink {
        duplicate_groups: Arc::new(Mutex::new(content_groups)),
        received_updates: Arc::new(Mutex::new(Vec::new())),
    };

    let summary = engine
        .run_universal_classification(&sink, &config, false)
        .await
        .unwrap();

    assert_eq!(summary.total_files_scanned, 3);
    assert_eq!(summary.unique_media_count, 1);
    assert_eq!(summary.duplicate_clusters_count, 1);
    assert_eq!(summary.exact_duplicate_files, 1);
    assert_eq!(summary.total_reclaimable_bytes, 4_000_000);
    assert_eq!(sink.received_updates.lock().unwrap().len(), 3);
}

#[test]
fn test_evaluate_replica_cluster_conflict_in_master() {
    let config = create_test_config();
    let matcher = CanonicalPathMatcher::new(config);

    // Two identical files inside the primary master under different dates
    let doc_master_1 = create_mock_doc(
        "/Volumes/SP-1/SPSA/media/master/2021/30-Nov-2021/Photos/IMG_100.JPG",
        12_000_000,
        "hash_conflict",
    );
    let doc_master_2 = create_mock_doc(
        "/Volumes/SP-1/SPSA/media/master/2021/01-Dec-2021/Photos/IMG_100.JPG",
        12_000_000,
        "hash_conflict",
    );

    let cluster = vec![doc_master_1.clone(), doc_master_2.clone()];
    let results = MediaDeduplicationEngine::evaluate_replica_cluster(&cluster, &matcher);

    assert_eq!(results.len(), 2);
    for (_id, meta) in results {
        assert_eq!(meta.status, DeduplicationStatus::ConflictReview);
        assert_eq!(meta.reclaimable_bytes, 0);
    }
}

#[tokio::test]
async fn test_run_database_dedup_dry_run_and_execution() {
    let config = create_test_config();
    let engine = MediaDeduplicationEngine::new();

    let doc_master = create_mock_doc(
        "/Volumes/SP-1/SPSA/media/master/2021/30-Nov-2021/Photos/IMG_500.JPG",
        10_000_000,
        "hash_run_test",
    );
    let doc_redundant = create_mock_doc(
        "/Volumes/SP-1/temp_junk/IMG_500.JPG",
        10_000_000,
        "hash_run_test",
    );

    let sink = MockStorageSink::default();
    sink.duplicate_groups.lock().unwrap().push((
        "hash_run_test".to_string(),
        vec![doc_master.clone(), doc_redundant.clone()],
    ));

    // Test 1: Dry-run = true
    let summary_dry = engine
        .run_database_dedup(&sink, &config, true)
        .await
        .unwrap();

    assert_eq!(summary_dry.total_files_scanned, 2);
    assert_eq!(summary_dry.duplicate_clusters_count, 1);
    assert_eq!(summary_dry.exact_duplicate_files, 1);
    assert_eq!(summary_dry.total_reclaimable_bytes, 10_000_000);
    assert_eq!(summary_dry.unique_media_count, 1);
    // In dry-run, no database writes should have occurred!
    assert!(sink.received_updates.lock().unwrap().is_empty());

    // Test 2: Dry-run = false (Execution)
    let summary_exec = engine
        .run_database_dedup(&sink, &config, false)
        .await
        .unwrap();

    assert_eq!(summary_exec.exact_duplicate_files, 1);
    let updates = sink.received_updates.lock().unwrap().clone();
    assert_eq!(updates.len(), 2);

    let update_map: HashMap<Uuid, DeduplicationMetadata> = updates.into_iter().collect();
    assert_eq!(
        update_map.get(&doc_master.id).unwrap().status,
        DeduplicationStatus::CanonicalMaster
    );
    assert_eq!(
        update_map.get(&doc_redundant.id).unwrap().status,
        DeduplicationStatus::MarkForRemoval
    );
}
