use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use uuid::Uuid;

use metadata_extractor::application::replica_service::{ReplicaSyncOptions, ReplicaSyncService};
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
use metadata_extractor::domain::ports::{BatchWriteStats, MediaFingerprinter, MetadataStorageSink};
use metadata_extractor::infrastructure::blake3_hasher::Blake3MediaHasher;

#[derive(Default, Clone)]
pub struct MockReplicaStorageSink {
    pub documents: Arc<Mutex<HashMap<Uuid, MediaMetadataDocument>>>,
}

#[async_trait::async_trait]
impl MetadataStorageSink for MockReplicaStorageSink {
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
        let mut guard = self.documents.lock().unwrap();
        for doc in docs {
            guard.insert(doc.id, doc.clone());
        }
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
        Ok(Vec::new())
    }

    async fn update_dedup_batch(
        &self,
        updates: &[(Uuid, DeduplicationMetadata)],
    ) -> Result<usize, StorageError> {
        let mut guard = self.documents.lock().unwrap();
        for (id, meta) in updates {
            if let Some(doc) = guard.get_mut(id) {
                doc.dedup = Some(meta.clone());
            }
        }
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
        doc_id: Uuid,
    ) -> Result<Option<MediaMetadataDocument>, StorageError> {
        let guard = self.documents.lock().unwrap();
        Ok(guard.get(&doc_id).cloned())
    }

    async fn mark_removed(&self, doc_id: Uuid) -> Result<(), StorageError> {
        let mut guard = self.documents.lock().unwrap();
        if let Some(doc) = guard.get_mut(&doc_id) {
            if let Some(ref mut d) = doc.dedup {
                d.status = DeduplicationStatus::Removed;
            }
        }
        Ok(())
    }

    async fn query_all_content_groups(
        &self,
        _mount_prefix: Option<&str>,
    ) -> Result<Vec<(String, Vec<MediaMetadataDocument>)>, StorageError> {
        let guard = self.documents.lock().unwrap();
        let mut groups: HashMap<String, Vec<MediaMetadataDocument>> = HashMap::new();
        for doc in guard.values() {
            if let Some(ref fp) = doc.fingerprint {
                groups
                    .entry(fp.cryptographic_hash.clone())
                    .or_default()
                    .push(doc.clone());
            }
        }
        Ok(groups.into_iter().collect())
    }

    async fn query_canonical_masters(&self) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        let guard = self.documents.lock().unwrap();
        let masters = guard
            .values()
            .filter(|d| {
                d.dedup
                    .as_ref()
                    .map(|meta| meta.status == DeduplicationStatus::CanonicalMaster)
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        Ok(masters)
    }

    async fn query_replica_documents(
        &self,
        replica_drive_id: &str,
    ) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        let guard = self.documents.lock().unwrap();
        let replicas = guard
            .values()
            .filter(|d| {
                d.dedup
                    .as_ref()
                    .map(|meta| {
                        meta.status == DeduplicationStatus::ReplicaKeep
                            && meta.drive_id.as_deref() == Some(replica_drive_id)
                    })
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        Ok(replicas)
    }
}

fn create_canonical_master_doc(
    id: Uuid,
    path: PathBuf,
    size: u64,
    cat: MediaCategory,
    dt: DateTime<Utc>,
    hash: String,
) -> MediaMetadataDocument {
    MediaMetadataDocument {
        id,
        file_context: FileContext {
            scanned_root: path.parent().unwrap().to_path_buf(),
            file_path: path.clone(),
            relative_path: path.clone(),
            file_name: path.file_name().unwrap().to_string_lossy().to_string(),
            file_size_bytes: size,
            extension: "jpg".to_string(),
            media_category: cat,
            modified_at: Some(dt),
        },
        temporal: TemporalMetadata::from_utc_datetime(dt, TimestampSource::ExifDateTimeOriginal),
        camera: None,
        exposure: None,
        location: None,
        dimensions: None,
        video: None,
        fingerprint: Some(MediaFingerprint {
            id: Uuid::new_v4(),
            file_path: path.clone(),
            file_size_bytes: size,
            media_category: cat,
            dimensions: None,
            sample_digest: None,
            cryptographic_hash: hash,
            perceptual_hash: None,
            phash_hex: None,
            rotated_hashes: None,
            is_low_entropy: false,
            fingerprinted_at: dt,
        }),
        extraction_status: ExtractionStatus::default(),
        dedup: Some(DeduplicationMetadata {
            cluster_id: Uuid::new_v4(),
            status: DeduplicationStatus::CanonicalMaster,
            canonical_master_id: Some(id),
            canonical_master_path: Some(path),
            drive_id: Some("primary_main".to_string()),
            drive_group: Some("primary".to_string()),
            is_in_master_path: true,
            reclaimable_bytes: 0,
            evaluated_at: dt,
        }),
    }
}

#[tokio::test]
async fn test_replica_sync_fanout_and_verification() {
    let temp_root = tempdir().unwrap();
    let primary_mount = temp_root.path().join("PrimaryDrive");
    let primary_master = primary_mount.join("SPSA/media/master");
    let replica1_mount = temp_root.path().join("Replica1");
    let replica1_master = replica1_mount.join("SPSA/media/master");
    let replica2_mount = temp_root.path().join("Replica2");
    let replica2_master = replica2_mount.join("SPSA/media/master");

    std::fs::create_dir_all(&primary_master).unwrap();
    std::fs::create_dir_all(&replica1_master).unwrap();
    std::fs::create_dir_all(&replica2_master).unwrap();

    let mut groups = HashMap::new();
    groups.insert(
        "primary".to_string(),
        DriveGroupSpec {
            priority: 100,
            description: None,
        },
    );
    groups.insert(
        "replica1".to_string(),
        DriveGroupSpec {
            priority: 80,
            description: None,
        },
    );
    groups.insert(
        "replica2".to_string(),
        DriveGroupSpec {
            priority: 70,
            description: None,
        },
    );

    let config = DriveConfiguration {
        version: "1.0".to_string(),
        groups,
        drives: vec![
            DriveDefinition {
                id: "primary_main".to_string(),
                group: "primary".to_string(),
                mount_path: primary_mount.clone(),
                master_path: primary_master.clone(),
            },
            DriveDefinition {
                id: "rep_1".to_string(),
                group: "replica1".to_string(),
                mount_path: replica1_mount.clone(),
                master_path: replica1_master.clone(),
            },
            DriveDefinition {
                id: "rep_2".to_string(),
                group: "replica2".to_string(),
                mount_path: replica2_mount.clone(),
                master_path: replica2_master.clone(),
            },
        ],
        rules: DriveRules::default(),
    };

    // Create a CanonicalMaster file on Primary
    let master_file_dir = primary_master.join("2024/15-Aug-2024/Photos");
    std::fs::create_dir_all(&master_file_dir).unwrap();
    let master_file = master_file_dir.join("vacation.jpg");
    std::fs::write(
        &master_file,
        b"Canonical vacation photo bytes on primary master",
    )
    .unwrap();

    let hasher = Blake3MediaHasher::new();
    let hash = hasher.compute_cryptographic_hash(&master_file).unwrap();

    let master_id = Uuid::new_v4();
    let dt = DateTime::parse_from_rfc3339("2024-08-15T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    let master_doc = create_canonical_master_doc(
        master_id,
        master_file.clone(),
        master_file.metadata().unwrap().len(),
        MediaCategory::Photo,
        dt,
        hash.clone(),
    );

    let sink = MockReplicaStorageSink::default();
    sink.save_batch(&[master_doc]).await.unwrap();

    let service = ReplicaSyncService::new();
    let options = ReplicaSyncOptions {
        target_replica_id: None,
        primary_drive_id: None,
        dry_run: false,
        verify_hash: true,
    };

    // 1. Dry run verification
    let mut dry_options = options.clone();
    dry_options.dry_run = true;
    let dry_report = service
        .execute_sync(&sink, &config, dry_options)
        .await
        .unwrap();

    assert!(dry_report.is_dry_run);
    assert_eq!(dry_report.total_files_planned, 2); // 1 file * 2 replicas
    assert_eq!(dry_report.files_replicated, 0);

    // 2. Real execution
    let report = service.execute_sync(&sink, &config, options).await.unwrap();
    assert!(!report.is_dry_run);
    assert_eq!(report.files_replicated, 2);
    assert_eq!(report.files_failed, 0);
    assert_eq!(report.errors.len(), 0);

    // Verify files copied to both replica drives with exact relative path
    let rep1_file = replica1_master.join("2024/15-Aug-2024/Photos/vacation.jpg");
    let rep2_file = replica2_master.join("2024/15-Aug-2024/Photos/vacation.jpg");
    assert!(rep1_file.exists());
    assert!(rep2_file.exists());
    assert_eq!(hasher.compute_cryptographic_hash(&rep1_file).unwrap(), hash);
    assert_eq!(hasher.compute_cryptographic_hash(&rep2_file).unwrap(), hash);

    // Verify catalog has 2 new ReplicaKeep records
    let rep1_docs = sink.query_replica_documents("rep_1").await.unwrap();
    let rep2_docs = sink.query_replica_documents("rep_2").await.unwrap();
    assert_eq!(rep1_docs.len(), 1);
    assert_eq!(rep2_docs.len(), 1);

    let r1_meta = rep1_docs[0].dedup.as_ref().unwrap();
    assert_eq!(r1_meta.status, DeduplicationStatus::ReplicaKeep);
    assert_eq!(r1_meta.canonical_master_id, Some(master_id));
    assert!(r1_meta.is_in_master_path);

    let r2_meta = rep2_docs[0].dedup.as_ref().unwrap();
    assert_eq!(r2_meta.status, DeduplicationStatus::ReplicaKeep);
    assert_eq!(r2_meta.canonical_master_id, Some(master_id));
}

#[tokio::test]
async fn test_replica_sync_offline_drive_tolerance() {
    let temp_root = tempdir().unwrap();
    let primary_mount = temp_root.path().join("PrimaryDrive");
    let primary_master = primary_mount.join("SPSA/media/master");
    let replica1_mount = temp_root.path().join("Replica1");
    let replica1_master = replica1_mount.join("SPSA/media/master");
    // Replica2 mount path intentionally NOT created (offline / detached)
    let replica2_mount = temp_root.path().join("UnmountedDrive_Offline");
    let replica2_master = replica2_mount.join("SPSA/media/master");

    std::fs::create_dir_all(&primary_master).unwrap();
    std::fs::create_dir_all(&replica1_master).unwrap();

    let mut groups = HashMap::new();
    groups.insert(
        "primary".to_string(),
        DriveGroupSpec {
            priority: 100,
            description: None,
        },
    );
    groups.insert(
        "replica1".to_string(),
        DriveGroupSpec {
            priority: 80,
            description: None,
        },
    );
    groups.insert(
        "replica2".to_string(),
        DriveGroupSpec {
            priority: 70,
            description: None,
        },
    );

    let config = DriveConfiguration {
        version: "1.0".to_string(),
        groups,
        drives: vec![
            DriveDefinition {
                id: "primary_main".to_string(),
                group: "primary".to_string(),
                mount_path: primary_mount.clone(),
                master_path: primary_master.clone(),
            },
            DriveDefinition {
                id: "rep_1".to_string(),
                group: "replica1".to_string(),
                mount_path: replica1_mount.clone(),
                master_path: replica1_master.clone(),
            },
            DriveDefinition {
                id: "rep_2_offline".to_string(),
                group: "replica2".to_string(),
                mount_path: replica2_mount.clone(),
                master_path: replica2_master.clone(),
            },
        ],
        rules: DriveRules::default(),
    };

    let master_file_dir = primary_master.join("2024/15-Aug-2024/Photos");
    std::fs::create_dir_all(&master_file_dir).unwrap();
    let master_file = master_file_dir.join("offline_test.jpg");
    std::fs::write(
        &master_file,
        b"Primary file testing offline replica tolerance",
    )
    .unwrap();

    let hasher = Blake3MediaHasher::new();
    let hash = hasher.compute_cryptographic_hash(&master_file).unwrap();
    let master_id = Uuid::new_v4();
    let dt = DateTime::parse_from_rfc3339("2024-08-15T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    let master_doc = create_canonical_master_doc(
        master_id,
        master_file.clone(),
        master_file.metadata().unwrap().len(),
        MediaCategory::Photo,
        dt,
        hash,
    );

    let sink = MockReplicaStorageSink::default();
    sink.save_batch(&[master_doc]).await.unwrap();

    let service = ReplicaSyncService::new();
    let options = ReplicaSyncOptions {
        target_replica_id: None,
        primary_drive_id: None,
        dry_run: false,
        verify_hash: true,
    };

    let report = service.execute_sync(&sink, &config, options).await.unwrap();

    // Connected replica synced successfully
    assert_eq!(report.files_replicated, 1);
    assert_eq!(report.synchronized_replicas, vec!["rep_1".to_string()]);

    // Offline replica safely tracked without crashing
    assert_eq!(report.offline_replicas, vec!["rep_2_offline".to_string()]);
    assert_eq!(report.errors.len(), 0);
}
