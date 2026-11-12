use chrono::Utc;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use uuid::Uuid;

use metadata_extractor::application::drive_pruner::{DrivePrunerOptions, DrivePruningService};
use metadata_extractor::domain::dedup_models::{
    DeduplicationMetadata, DeduplicationStatus, DriveConfiguration, DriveDefinition,
    DriveGroupSpec, DriveRules,
};
use metadata_extractor::domain::errors::StorageError;
use metadata_extractor::domain::metadata_models::{
    ExtractionStatus, FileContext, FilterEvaluationResult, MediaMetadataDocument, TemporalMetadata,
    TimestampSource,
};
use metadata_extractor::domain::models::{DiscoveredMediaItem, MediaCategory};
use metadata_extractor::domain::ports::{BatchWriteStats, MetadataStorageSink};

#[derive(Default, Clone)]
struct MockPruneStorageSink {
    documents: Arc<Mutex<HashMap<Uuid, MediaMetadataDocument>>>,
    marked_removed: Arc<Mutex<Vec<Uuid>>>,
}

#[async_trait::async_trait]
impl MetadataStorageSink for MockPruneStorageSink {
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
        _updates: &[(Uuid, DeduplicationMetadata)],
    ) -> Result<usize, StorageError> {
        Ok(0)
    }

    async fn query_removal_candidates(
        &self,
        mount_prefix: Option<&str>,
    ) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        let guard = self.documents.lock().unwrap();
        let candidates = guard
            .values()
            .filter(|doc| {
                if let Some(ref d) = doc.dedup {
                    if d.status != DeduplicationStatus::MarkForRemoval {
                        return false;
                    }
                } else {
                    return false;
                }

                if let Some(prefix) = mount_prefix {
                    doc.file_context
                        .file_path
                        .to_string_lossy()
                        .starts_with(prefix)
                } else {
                    true
                }
            })
            .cloned()
            .collect();

        Ok(candidates)
    }

    async fn query_unmastered_candidates(
        &self,
        mount_prefix: Option<&str>,
    ) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        let guard = self.documents.lock().unwrap();
        let candidates = guard
            .values()
            .filter(|doc| {
                if let Some(ref d) = doc.dedup {
                    if d.status != DeduplicationStatus::NeedsIngestToMaster {
                        return false;
                    }
                } else {
                    return false;
                }

                if let Some(prefix) = mount_prefix {
                    doc.file_context
                        .file_path
                        .to_string_lossy()
                        .starts_with(prefix)
                } else {
                    true
                }
            })
            .cloned()
            .collect();

        Ok(candidates)
    }

    async fn find_document_by_id(
        &self,
        doc_id: Uuid,
    ) -> Result<Option<MediaMetadataDocument>, StorageError> {
        let guard = self.documents.lock().unwrap();
        Ok(guard.get(&doc_id).cloned())
    }

    async fn mark_removed(&self, doc_id: Uuid) -> Result<(), StorageError> {
        self.marked_removed.lock().unwrap().push(doc_id);
        Ok(())
    }

    async fn query_all_content_groups(
        &self,
        _mount_prefix: Option<&str>,
    ) -> Result<Vec<(String, Vec<MediaMetadataDocument>)>, StorageError> {
        Ok(Vec::new())
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
        _replica_drive_id: &str,
    ) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        Ok(Vec::new())
    }
}

fn create_doc_with_dedup(
    id: Uuid,
    path: PathBuf,
    size: u64,
    status: DeduplicationStatus,
    master_id: Option<Uuid>,
    master_path: Option<PathBuf>,
) -> MediaMetadataDocument {
    let now = Utc::now();
    let filename = path.file_name().unwrap().to_string_lossy().to_string();

    MediaMetadataDocument {
        id,
        file_context: FileContext {
            scanned_root: PathBuf::from("/"),
            file_path: path,
            relative_path: PathBuf::from(&filename),
            file_name: filename,
            file_size_bytes: size,
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
        fingerprint: None,
        extraction_status: ExtractionStatus::default(),
        dedup: Some(DeduplicationMetadata {
            cluster_id: Uuid::new_v4(),
            status,
            canonical_master_id: master_id,
            canonical_master_path: master_path,
            drive_id: Some("drive_test".to_string()),
            drive_group: Some("primary".to_string()),
            is_in_master_path: status == DeduplicationStatus::CanonicalMaster,
            reclaimable_bytes: if status == DeduplicationStatus::MarkForRemoval {
                size
            } else {
                0
            },
            evaluated_at: now,
        }),
    }
}

#[tokio::test]
async fn test_drive_pruner_manifest_and_safety_checks() {
    let temp_root = tempdir().unwrap();
    let mount_dir = temp_root.path().join("Drive3");
    std::fs::create_dir_all(&mount_dir).unwrap();

    let mut groups = HashMap::new();
    groups.insert(
        "primary".to_string(),
        DriveGroupSpec {
            priority: 100,
            description: None,
        },
    );

    let config = DriveConfiguration {
        version: "1.0".to_string(),
        groups,
        drives: vec![DriveDefinition {
            id: "drive_3".to_string(),
            group: "primary".to_string(),
            mount_path: mount_dir.clone(),
            master_path: mount_dir.join("master"),
        }],
        rules: DriveRules::default(),
    };

    let service = DrivePruningService::new();
    let sink = MockPruneStorageSink::default();

    // 1. Create a Master Document
    let master_id = Uuid::new_v4();
    let master_path = mount_dir.join("master/2021/30-Nov-2021/Photos/IMG_1.JPG");
    let master_doc = create_doc_with_dedup(
        master_id,
        master_path.clone(),
        1000,
        DeduplicationStatus::CanonicalMaster,
        Some(master_id),
        Some(master_path.clone()),
    );
    sink.save_batch(&[master_doc]).await.unwrap();

    // 2. Candidate with VERIFIED master
    let cand1_id = Uuid::new_v4();
    let cand1_path = mount_dir.join("scattered/IMG_1_dup.JPG");
    std::fs::create_dir_all(cand1_path.parent().unwrap()).unwrap();
    std::fs::write(&cand1_path, b"dummy content").unwrap();

    let cand1_doc = create_doc_with_dedup(
        cand1_id,
        cand1_path.clone(),
        1000,
        DeduplicationStatus::MarkForRemoval,
        Some(master_id),
        Some(master_path.clone()),
    );

    // 3. Candidate with UNVERIFIED master (master doc ID does not exist in DB!)
    let unverified_master_id = Uuid::new_v4();
    let cand2_id = Uuid::new_v4();
    let cand2_path = mount_dir.join("scattered/IMG_unverified.JPG");
    std::fs::write(&cand2_path, b"unverified content").unwrap();

    let cand2_doc = create_doc_with_dedup(
        cand2_id,
        cand2_path.clone(),
        2000,
        DeduplicationStatus::MarkForRemoval,
        Some(unverified_master_id),
        Some(PathBuf::from("/nonexistent/master.jpg")),
    );

    sink.save_batch(&[cand1_doc, cand2_doc]).await.unwrap();

    // Test Dry-Run Pruning
    let manifest_path = temp_root.path().join("manifest.json");
    let dry_run_opts = DrivePrunerOptions {
        drive_identifier: "drive_3".to_string(),
        execute: false,
        require_master_disk: false,
        manifest_out: Some(manifest_path.clone()),
    };

    let report_dry = service
        .prune_drive(&sink, &config, dry_run_opts)
        .await
        .unwrap();

    assert!(report_dry.is_dry_run);
    assert_eq!(report_dry.total_candidates, 2);
    assert_eq!(report_dry.verified_masters, 1);
    assert_eq!(report_dry.unverified_masters, 1);
    assert_eq!(report_dry.files_deleted, 0);
    // Files on disk must still exist!
    assert!(cand1_path.exists());
    assert!(cand2_path.exists());
    assert!(manifest_path.exists());

    // Test Execute Pruning
    let exec_opts = DrivePrunerOptions {
        drive_identifier: "drive_3".to_string(),
        execute: true,
        require_master_disk: false,
        manifest_out: None,
    };

    let report_exec = service
        .prune_drive(&sink, &config, exec_opts)
        .await
        .unwrap();

    assert!(!report_exec.is_dry_run);
    assert_eq!(report_exec.files_deleted, 1);
    assert_eq!(report_exec.bytes_reclaimed, 1000);
    // cand1 (verified) MUST be deleted!
    assert!(!cand1_path.exists());
    // cand2 (unverified master) MUST NOT be deleted (safety invariant)!
    assert!(cand2_path.exists());

    // Check DB was updated with mark_removed for cand1 only
    let removed = sink.marked_removed.lock().unwrap().clone();
    assert_eq!(removed, vec![cand1_id]);
}

#[tokio::test]
async fn test_drive_pruner_fallback_to_hash_verification() {
    let temp_root = tempdir().unwrap();
    let mount_dir = temp_root.path().join("DriveStaging");
    let master_dir = temp_root.path().join("DriveMaster");
    std::fs::create_dir_all(&mount_dir).unwrap();
    std::fs::create_dir_all(&master_dir).unwrap();

    let config = DriveConfiguration {
        version: "1.0".to_string(),
        groups: HashMap::new(),
        drives: vec![
            DriveDefinition {
                id: "staging".to_string(),
                group: "staging".to_string(),
                mount_path: mount_dir.clone(),
                master_path: mount_dir.clone(),
            },
            DriveDefinition {
                id: "master".to_string(),
                group: "primary".to_string(),
                mount_path: master_dir.clone(),
                master_path: master_dir.clone(),
            },
        ],
        rules: DriveRules::default(),
    };

    let service = DrivePruningService::new();
    let sink = MockPruneStorageSink::default();

    // 1. Create a CanonicalMaster document with hash
    let master_path = master_dir.join("photo.jpg");
    std::fs::write(&master_path, b"canonical master photo bytes").unwrap();
    let mut master_doc = create_doc_with_dedup(
        Uuid::new_v4(),
        master_path.clone(),
        1000,
        DeduplicationStatus::CanonicalMaster,
        None,
        Some(master_path.clone()),
    );
    master_doc.fingerprint = Some(metadata_extractor::domain::dedup_models::MediaFingerprint {
        id: Uuid::new_v4(),
        file_path: master_path.clone(),
        file_size_bytes: 1000,
        media_category: MediaCategory::Photo,
        dimensions: None,
        sample_digest: Some("hash123".to_string()),
        cryptographic_hash: "blake3_unique_hash_xyz".to_string(),
        perceptual_hash: None,
        phash_hex: None,
        rotated_hashes: None,
        is_low_entropy: false,
        fingerprinted_at: Utc::now(),
    });

    // 2. Create duplicate candidate on staging with canonical_master_id = None
    let cand_path = mount_dir.join("photo_copy.jpg");
    std::fs::write(&cand_path, b"canonical master photo bytes").unwrap();
    let mut cand_doc = create_doc_with_dedup(
        Uuid::new_v4(),
        cand_path.clone(),
        1000,
        DeduplicationStatus::MarkForRemoval,
        None, // canonical_master_id is None!
        None, // canonical_master_path is None!
    );
    cand_doc.fingerprint = Some(metadata_extractor::domain::dedup_models::MediaFingerprint {
        id: Uuid::new_v4(),
        file_path: cand_path.clone(),
        file_size_bytes: 1000,
        media_category: MediaCategory::Photo,
        dimensions: None,
        sample_digest: Some("hash123".to_string()),
        cryptographic_hash: "blake3_unique_hash_xyz".to_string(), // Matches master hash!
        perceptual_hash: None,
        phash_hex: None,
        rotated_hashes: None,
        is_low_entropy: false,
        fingerprinted_at: Utc::now(),
    });

    sink.save_batch(&[master_doc, cand_doc.clone()])
        .await
        .unwrap();

    let exec_opts = DrivePrunerOptions {
        drive_identifier: "staging".to_string(),
        execute: true,
        require_master_disk: true,
        manifest_out: None,
    };

    let report = service
        .prune_drive(&sink, &config, exec_opts)
        .await
        .unwrap();

    assert_eq!(report.files_deleted, 1);
    assert_eq!(report.verified_masters, 1);
    assert_eq!(report.unverified_masters, 0);
    assert!(!cand_path.exists());
}
