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
        if let Some(doc) = self.documents.lock().unwrap().get_mut(&doc_id) {
            if let Some(ref mut d) = doc.dedup {
                d.status = DeduplicationStatus::Removed;
            }
        }
        Ok(())
    }

    async fn delete_document(&self, doc_id: Uuid) -> Result<bool, StorageError> {
        let mut guard = self.documents.lock().unwrap();
        if let Some(doc) = guard.get(&doc_id) {
            if let Some(ref d) = doc.dedup {
                if d.status == DeduplicationStatus::CanonicalMaster
                    || d.status == DeduplicationStatus::ReplicaKeep
                {
                    return Ok(false);
                }
            }
        }
        Ok(guard.remove(&doc_id).is_some())
    }

    async fn delete_documents(&self, doc_ids: &[Uuid]) -> Result<usize, StorageError> {
        let mut count = 0;
        for id in doc_ids {
            if self.delete_document(*id).await? {
                count += 1;
            }
        }
        Ok(count)
    }

    async fn purge_removed_records(
        &self,
        mount_prefix: Option<&str>,
    ) -> Result<usize, StorageError> {
        let mut guard = self.documents.lock().unwrap();
        let to_remove: Vec<Uuid> = guard
            .values()
            .filter(|doc| {
                if let Some(ref d) = doc.dedup {
                    if d.status != DeduplicationStatus::Removed {
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
                        || doc
                            .file_context
                            .scanned_root
                            .to_string_lossy()
                            .starts_with(prefix)
                } else {
                    true
                }
            })
            .map(|d| d.id)
            .collect();

        for id in &to_remove {
            guard.remove(id);
        }
        Ok(to_remove.len())
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
            replication: None,
        }),
        visual: None,
    }
}

#[tokio::test]
async fn test_drive_pruner_manifest_and_safety_checks() {
    let temp_root = tempdir().unwrap();
    let primary_dir = temp_root.path().join("Primary");
    let mount_dir = temp_root.path().join("Drive3");
    std::fs::create_dir_all(&primary_dir).unwrap();
    std::fs::create_dir_all(&mount_dir).unwrap();

    let mut groups = HashMap::new();
    groups.insert(
        "primary".to_string(),
        DriveGroupSpec {
            priority: 100,
            description: None,
        },
    );
    groups.insert(
        "staging".to_string(),
        DriveGroupSpec {
            priority: 20,
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
                mount_path: primary_dir.clone(),
                master_path: primary_dir.join("master"),
            },
            DriveDefinition {
                id: "drive_3".to_string(),
                group: "staging".to_string(),
                mount_path: mount_dir.clone(),
                master_path: mount_dir.join("staging"),
            },
        ],
        rules: DriveRules::default(),
    };

    let service = DrivePruningService::new();
    let sink = MockPruneStorageSink::default();

    // 1. Create a Master Document on primary
    let master_id = Uuid::new_v4();
    let master_path = primary_dir.join("master/2021/30-Nov-2021/Photos/IMG_1.JPG");
    std::fs::create_dir_all(master_path.parent().unwrap()).unwrap();
    std::fs::write(&master_path, b"canonical master bytes").unwrap();
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
        drive_identifier: Some("drive_3".to_string()),
        execute: false,
        require_master_disk: false,
        manifest_out: Some(manifest_path.clone()),
        ..Default::default()
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
        drive_identifier: Some("drive_3".to_string()),
        execute: true,
        require_master_disk: false,
        manifest_out: None,
        ..Default::default()
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

    // Check DB record was purged for cand1 only
    assert_eq!(report_exec.records_purged, 1);
    assert!(sink.find_document_by_id(cand1_id).await.unwrap().is_none());
    assert!(sink.find_document_by_id(cand2_id).await.unwrap().is_some());
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
        drive_identifier: Some("staging".to_string()),
        execute: true,
        require_master_disk: true,
        manifest_out: None,
        ..Default::default()
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

fn make_multi_drive_config(
    primary_dir: &std::path::Path,
    replica_dir: &std::path::Path,
    staging_dir: &std::path::Path,
) -> DriveConfiguration {
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
        "staging".to_string(),
        DriveGroupSpec {
            priority: 20,
            description: None,
        },
    );

    DriveConfiguration {
        version: "1.0".to_string(),
        groups,
        drives: vec![
            DriveDefinition {
                id: "primary_main".to_string(),
                mount_path: primary_dir.to_path_buf(),
                master_path: primary_dir.join("media/master"),
                group: "primary".to_string(),
            },
            DriveDefinition {
                id: "replica_1".to_string(),
                mount_path: replica_dir.to_path_buf(),
                master_path: replica_dir.join("media/master"),
                group: "replica1".to_string(),
            },
            DriveDefinition {
                id: "staging".to_string(),
                mount_path: staging_dir.to_path_buf(),
                master_path: staging_dir.join("media/staging"),
                group: "staging".to_string(),
            },
        ],
        rules: DriveRules::default(),
    }
}

#[tokio::test]
async fn test_prune_drive_rejects_primary_and_replica_targets() {
    let tmp = tempdir().unwrap();
    let primary_dir = tmp.path().join("primary");
    let replica_dir = tmp.path().join("replica");
    let staging_dir = tmp.path().join("staging");

    std::fs::create_dir_all(&primary_dir).unwrap();
    std::fs::create_dir_all(&replica_dir).unwrap();
    std::fs::create_dir_all(&staging_dir).unwrap();

    let config = make_multi_drive_config(&primary_dir, &replica_dir, &staging_dir);
    let sink = MockPruneStorageSink::default();
    let service = DrivePruningService::new();

    // 1. Target primary canonical master path directly -> STRICTLY FORBIDDEN
    let master_path = primary_dir.join("media/master");
    std::fs::create_dir_all(&master_path).unwrap();
    let opts_primary_master = DrivePrunerOptions {
        drive_identifier: Some(master_path.to_str().unwrap().to_string()),
        execute: false,
        require_master_disk: false,
        manifest_out: None,
        ..Default::default()
    };
    let err_primary = service
        .prune_drive(&sink, &config, opts_primary_master)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err_primary,
            metadata_extractor::domain::errors::DeduplicationError::PruneForbiddenOnProtectedArchive { .. }
        ),
        "Expected PruneForbiddenOnProtectedArchive, got {:?}",
        err_primary
    );

    // 2. Target replica master path directly -> STRICTLY FORBIDDEN
    let replica_master_path = replica_dir.join("media/master");
    std::fs::create_dir_all(&replica_master_path).unwrap();
    let opts_replica_master = DrivePrunerOptions {
        drive_identifier: Some(replica_master_path.to_str().unwrap().to_string()),
        execute: false,
        require_master_disk: false,
        manifest_out: None,
        ..Default::default()
    };
    let err_replica = service
        .prune_drive(&sink, &config, opts_replica_master)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err_replica,
            metadata_extractor::domain::errors::DeduplicationError::PruneForbiddenOnProtectedArchive { .. }
        ),
        "Expected PruneForbiddenOnProtectedArchive, got {:?}",
        err_replica
    );

    // 3. Target unorganized directory on primary drive -> ALLOWED
    let unorg_path = primary_dir.join("unorganized");
    std::fs::create_dir_all(&unorg_path).unwrap();
    let opts_unorg = DrivePrunerOptions {
        drive_identifier: Some(unorg_path.to_str().unwrap().to_string()),
        execute: false,
        require_master_disk: false,
        manifest_out: None,
        ..Default::default()
    };
    let res_unorg = service.prune_drive(&sink, &config, opts_unorg).await;
    assert!(
        res_unorg.is_ok(),
        "Targeting unorganized folder on primary must be permitted"
    );

    // 4. Target primary drive ID where mount_path != master_path -> ALLOWED
    let opts_primary_drive = DrivePrunerOptions {
        drive_identifier: Some("primary_main".to_string()),
        execute: false,
        require_master_disk: false,
        manifest_out: None,
        ..Default::default()
    };
    let res_primary_drive = service
        .prune_drive(&sink, &config, opts_primary_drive)
        .await;
    assert!(
        res_primary_drive.is_ok(),
        "Targeting primary drive with unorganized space must be permitted"
    );
}

#[tokio::test]
async fn test_generate_manifest_disqualifies_protected_archive_candidates() {
    let tmp = tempdir().unwrap();
    let primary_dir = tmp.path().join("primary");
    let replica_dir = tmp.path().join("replica");
    let staging_dir = tmp.path().join("staging");

    std::fs::create_dir_all(&primary_dir).unwrap();
    std::fs::create_dir_all(&replica_dir).unwrap();
    std::fs::create_dir_all(&staging_dir).unwrap();

    let config = make_multi_drive_config(&primary_dir, &replica_dir, &staging_dir);
    let sink = MockPruneStorageSink::default();
    let service = DrivePruningService::new();

    // Create a physical file on staging
    let staging_file = staging_dir.join("staging_dup.jpg");
    std::fs::write(&staging_file, b"staging bytes").unwrap();

    // Create a physical file on primary master (simulate buggy DB entry marked MarkForRemoval)
    let primary_file = primary_dir.join("media/master/Photos/buggy_master.jpg");
    std::fs::create_dir_all(primary_file.parent().unwrap()).unwrap();
    std::fs::write(&primary_file, b"precious master bytes").unwrap();

    let doc_staging = create_doc_with_dedup(
        Uuid::new_v4(),
        staging_file.clone(),
        100,
        DeduplicationStatus::MarkForRemoval,
        Some(Uuid::new_v4()),
        Some(primary_file.clone()),
    );

    let doc_primary_bug = create_doc_with_dedup(
        Uuid::new_v4(),
        primary_file.clone(),
        100,
        DeduplicationStatus::MarkForRemoval,
        Some(Uuid::new_v4()),
        Some(primary_file.clone()),
    );

    sink.save_batch(&[doc_staging, doc_primary_bug])
        .await
        .unwrap();

    // Generate manifest for staging
    let manifest = service
        .generate_manifest(&sink, &config, Some("staging"), false)
        .await
        .unwrap();

    // The primary candidate must be completely disqualified and excluded
    assert_eq!(manifest.total_files_to_remove, 1);
    assert_eq!(manifest.removal_tasks[0].target_file_path, staging_file);
    assert!(primary_file.exists());
}

#[tokio::test]
async fn test_safe_delete_symlink_circuit_breaker() {
    let tmp = tempdir().unwrap();
    let primary_dir = tmp.path().join("primary");
    let replica_dir = tmp.path().join("replica");
    let staging_dir = tmp.path().join("staging");

    std::fs::create_dir_all(&primary_dir).unwrap();
    std::fs::create_dir_all(&replica_dir).unwrap();
    std::fs::create_dir_all(&staging_dir).unwrap();

    let config = make_multi_drive_config(&primary_dir, &replica_dir, &staging_dir);
    let guard =
        metadata_extractor::infrastructure::archive_guard::ArchiveProtectionGuard::from_config(
            &config,
        );

    // Primary real file
    let primary_file = primary_dir.join("media/master/Photos/real.jpg");
    std::fs::create_dir_all(primary_file.parent().unwrap()).unwrap();
    std::fs::write(&primary_file, b"real master bytes").unwrap();

    // Symlink on staging pointing to primary
    let symlink_path = staging_dir.join("symlink_to_master.jpg");
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&primary_file, &symlink_path).unwrap();

        // safe_delete_file must refuse to delete through symlink
        let res = guard.safe_delete_file(&symlink_path);
        assert!(res.is_err());
        assert!(
            primary_file.exists(),
            "Target file on primary must remain untouched"
        );
    }
}

#[tokio::test]
async fn test_prune_drive_hard_deletes_database_records() {
    let tmp = tempdir().unwrap();
    let primary_dir = tmp.path().join("primary");
    let replica_dir = tmp.path().join("replica");
    let staging_dir = tmp.path().join("staging");

    std::fs::create_dir_all(&primary_dir).unwrap();
    std::fs::create_dir_all(&replica_dir).unwrap();
    std::fs::create_dir_all(&staging_dir).unwrap();

    let config = make_multi_drive_config(&primary_dir, &replica_dir, &staging_dir);
    let sink = MockPruneStorageSink::default();
    let service = DrivePruningService::new();

    // 1. Create a Master Document on primary
    let master_id = Uuid::new_v4();
    let master_path = primary_dir.join("media/master/Photos/IMG_MASTER.JPG");
    std::fs::create_dir_all(master_path.parent().unwrap()).unwrap();
    std::fs::write(&master_path, b"canonical master content").unwrap();
    let master_doc = create_doc_with_dedup(
        master_id,
        master_path.clone(),
        1000,
        DeduplicationStatus::CanonicalMaster,
        Some(master_id),
        Some(master_path.clone()),
    );
    sink.save_batch(&[master_doc]).await.unwrap();

    // 2. Create duplicate candidate on staging
    let cand_id = Uuid::new_v4();
    let cand_path = staging_dir.join("media/staging/IMG_DUP.JPG");
    std::fs::create_dir_all(cand_path.parent().unwrap()).unwrap();
    std::fs::write(&cand_path, b"canonical master content").unwrap();

    let cand_doc = create_doc_with_dedup(
        cand_id,
        cand_path.clone(),
        1000,
        DeduplicationStatus::MarkForRemoval,
        Some(master_id),
        Some(master_path.clone()),
    );
    sink.save_batch(&[cand_doc]).await.unwrap();

    // Verify candidate exists in DB before prune
    assert!(sink.find_document_by_id(cand_id).await.unwrap().is_some());

    // Execute prune
    let opts = DrivePrunerOptions {
        drive_identifier: Some("staging".to_string()),
        execute: true,
        require_master_disk: true,
        manifest_out: None,
        ..Default::default()
    };

    let report = service.prune_drive(&sink, &config, opts).await.unwrap();

    // Verify file deleted and record purged
    assert_eq!(report.files_deleted, 1);
    assert_eq!(report.records_purged, 1);
    assert!(
        !cand_path.exists(),
        "Duplicate file must be deleted from disk"
    );
    assert!(
        sink.find_document_by_id(cand_id).await.unwrap().is_none(),
        "Pruned duplicate record must be completely deleted from the database"
    );

    // Canonical master must remain completely intact
    assert!(
        sink.find_document_by_id(master_id).await.unwrap().is_some(),
        "Canonical master record must NEVER be deleted from the database"
    );
    assert!(
        master_path.exists(),
        "Canonical master file must remain on disk"
    );
}

#[tokio::test]
async fn test_prune_drive_purges_preexisting_removed_records() {
    let tmp = tempdir().unwrap();
    let primary_dir = tmp.path().join("primary");
    let replica_dir = tmp.path().join("replica");
    let staging_dir = tmp.path().join("staging");

    std::fs::create_dir_all(&primary_dir).unwrap();
    std::fs::create_dir_all(&replica_dir).unwrap();
    std::fs::create_dir_all(&staging_dir).unwrap();

    let config = make_multi_drive_config(&primary_dir, &replica_dir, &staging_dir);
    let sink = MockPruneStorageSink::default();
    let service = DrivePruningService::new();

    // Insert an obsolete record already marked as Removed from a historical run
    let old_removed_id = Uuid::new_v4();
    let old_removed_path = staging_dir.join("media/staging/old_debris.jpg");
    let old_removed_doc = create_doc_with_dedup(
        old_removed_id,
        old_removed_path,
        500,
        DeduplicationStatus::Removed,
        None,
        None,
    );
    sink.save_batch(&[old_removed_doc]).await.unwrap();

    // Verify it exists in DB before prune
    assert!(sink
        .find_document_by_id(old_removed_id)
        .await
        .unwrap()
        .is_some());

    // Execute prune on staging
    let opts = DrivePrunerOptions {
        drive_identifier: Some("staging".to_string()),
        execute: true,
        require_master_disk: false,
        manifest_out: None,
        ..Default::default()
    };

    let report = service.prune_drive(&sink, &config, opts).await.unwrap();

    // Pre-existing Removed record should be purged
    assert_eq!(report.records_purged, 1);
    assert!(
        sink.find_document_by_id(old_removed_id)
            .await
            .unwrap()
            .is_none(),
        "Pre-existing Removed records must be purged from database on prune execute"
    );
}

#[tokio::test]
async fn test_prune_drive_cleans_empty_ancestor_directories_and_preserves_root() {
    let tmp = tempdir().unwrap();
    let primary_dir = tmp.path().join("primary");
    let replica_dir = tmp.path().join("replica");
    let staging_root = tmp.path().join("staging");
    let unorganized_photos = staging_root.join("unorganized").join("Photos");

    std::fs::create_dir_all(&primary_dir).unwrap();
    std::fs::create_dir_all(&replica_dir).unwrap();

    // Nested folders that should become empty:
    let nested_trip = unorganized_photos
        .join("2021")
        .join("11-Nov-2021")
        .join("trip");
    std::fs::create_dir_all(&nested_trip).unwrap();

    // Sibling folder with a file that should NOT be deleted:
    let keep_folder = unorganized_photos.join("keep_folder");
    std::fs::create_dir_all(&keep_folder).unwrap();
    let keep_file = keep_folder.join("keep_me.jpg");
    std::fs::write(&keep_file, b"keep content").unwrap();

    // Master file on primary
    let master_file = primary_dir.join("media/master/2021/11-Nov-2021/Photos/trip_photo.jpg");
    std::fs::create_dir_all(master_file.parent().unwrap()).unwrap();
    std::fs::write(&master_file, b"photo content duplicate").unwrap();

    // Duplicate file in nested_trip
    let dup_file = nested_trip.join("trip_photo.jpg");
    std::fs::write(&dup_file, b"photo content duplicate").unwrap();

    // Put a .DS_Store file in nested_trip
    let ds_store = nested_trip.join(".DS_Store");
    std::fs::write(&ds_store, b"fake ds_store metadata").unwrap();

    let config = make_multi_drive_config(&primary_dir, &replica_dir, &staging_root);
    let sink = MockPruneStorageSink::default();
    let service = DrivePruningService::new();

    let master_id = Uuid::new_v4();
    let master_doc = create_doc_with_dedup(
        master_id,
        master_file.clone(),
        23,
        DeduplicationStatus::CanonicalMaster,
        None,
        None,
    );

    let dup_id = Uuid::new_v4();
    let dup_doc = create_doc_with_dedup(
        dup_id,
        dup_file.clone(),
        23,
        DeduplicationStatus::MarkForRemoval,
        Some(master_id),
        Some(master_file.clone()),
    );

    sink.save_batch(&[master_doc, dup_doc]).await.unwrap();

    // Target the specific directory: unorganized_photos
    let opts = DrivePrunerOptions {
        drive_identifier: Some(unorganized_photos.to_string_lossy().to_string()),
        execute: true,
        require_master_disk: true,
        manifest_out: None,
        ..Default::default()
    };

    let report = service.prune_drive(&sink, &config, opts).await.unwrap();

    assert_eq!(report.files_deleted, 1);
    assert_eq!(report.empty_directories_removed, 3); // trip, 11-Nov-2021, and 2021
    assert_eq!(report.errors.len(), 0);

    // Verify filesystem state:
    assert!(!dup_file.exists(), "Duplicate file must be deleted");
    assert!(!ds_store.exists(), ".DS_Store file must be removed");
    assert!(!nested_trip.exists(), "Empty trip folder must be removed");
    assert!(
        !unorganized_photos.join("2021").join("11-Nov-2021").exists(),
        "Empty 11-Nov-2021 folder must be removed"
    );
    assert!(
        !unorganized_photos.join("2021").exists(),
        "Empty 2021 folder must be removed"
    );

    // Target root must be preserved:
    assert!(
        unorganized_photos.exists(),
        "The target prune root directory itself must NEVER be deleted"
    );

    // Keep folder and file must be preserved:
    assert!(
        keep_folder.exists(),
        "Folder with non-duplicate files must be preserved"
    );
    assert!(
        keep_file.exists(),
        "Non-duplicate file must remain untouched"
    );
}

#[tokio::test]
async fn test_prune_drive_cleans_zero_byte_dummy_files_and_removes_empty_directories() {
    let tmp = tempdir().unwrap();
    let primary_dir = tmp.path().join("primary");
    let replica_dir = tmp.path().join("replica");
    let staging_root = tmp.path().join("staging");
    let photos_videos = staging_root.join("PhotosVideos");
    let dummy_folder = photos_videos.join("0256").join("01-January").join("Photos");

    std::fs::create_dir_all(&primary_dir).unwrap();
    std::fs::create_dir_all(&replica_dir).unwrap();
    std::fs::create_dir_all(&dummy_folder).unwrap();

    // Create a 0-byte corrupt/dummy file
    let zero_byte_file = dummy_folder.join("DSC_0256.JPG");
    std::fs::write(&zero_byte_file, b"").unwrap();
    assert_eq!(std::fs::metadata(&zero_byte_file).unwrap().len(), 0);

    let config = make_multi_drive_config(&primary_dir, &replica_dir, &staging_root);
    let sink = MockPruneStorageSink::default();
    let service = DrivePruningService::new();

    // Clean empty dirs on staging
    let opts = DrivePrunerOptions {
        drive_identifier: Some(staging_root.to_string_lossy().to_string()),
        execute: true,
        clean_empty_dirs: true,
        ..Default::default()
    };

    let report = service.prune_drive(&sink, &config, opts).await.unwrap();

    // 0-byte dummy file must be unlinked and empty directory hierarchy removed
    assert!(!zero_byte_file.exists(), "0-byte dummy file must be unlinked");
    assert!(!dummy_folder.exists(), "Empty dummy folder must be removed");
    assert!(!photos_videos.join("0256").exists(), "Empty 0256 folder must be removed");
    assert!(!photos_videos.exists(), "Empty PhotosVideos folder must be removed");
    assert!(staging_root.exists(), "Staging root mount must be preserved");
    assert_eq!(report.empty_directories_removed, 4); // Photos, 01-January, 0256, PhotosVideos
}

#[tokio::test]
async fn test_prune_drive_dry_run_simulates_empty_directories() {
    let tmp = tempdir().unwrap();
    let primary_dir = tmp.path().join("primary");
    let replica_dir = tmp.path().join("replica");
    let staging_root = tmp.path().join("staging");
    let unorganized_photos = staging_root.join("unorganized").join("Photos");

    std::fs::create_dir_all(&primary_dir).unwrap();
    std::fs::create_dir_all(&replica_dir).unwrap();

    let nested_trip = unorganized_photos
        .join("2022")
        .join("15-Dec-2022")
        .join("holiday");
    std::fs::create_dir_all(&nested_trip).unwrap();

    // Master file on primary
    let master_file = primary_dir.join("media/master/2022/15-Dec-2022/Photos/holiday.jpg");
    std::fs::create_dir_all(master_file.parent().unwrap()).unwrap();
    std::fs::write(&master_file, b"holiday photo content").unwrap();

    // Duplicate file in nested_trip
    let dup_file = nested_trip.join("holiday.jpg");
    std::fs::write(&dup_file, b"holiday photo content").unwrap();

    let config = make_multi_drive_config(&primary_dir, &replica_dir, &staging_root);
    let sink = MockPruneStorageSink::default();
    let service = DrivePruningService::new();

    let master_id = Uuid::new_v4();
    let master_doc = create_doc_with_dedup(
        master_id,
        master_file.clone(),
        21,
        DeduplicationStatus::CanonicalMaster,
        None,
        None,
    );

    let dup_id = Uuid::new_v4();
    let dup_doc = create_doc_with_dedup(
        dup_id,
        dup_file.clone(),
        21,
        DeduplicationStatus::MarkForRemoval,
        Some(master_id),
        Some(master_file.clone()),
    );

    sink.save_batch(&[master_doc, dup_doc]).await.unwrap();

    let opts = DrivePrunerOptions {
        drive_identifier: Some(unorganized_photos.to_string_lossy().to_string()),
        execute: false, // DRY-RUN
        require_master_disk: true,
        manifest_out: None,
        ..Default::default()
    };

    let report = service.prune_drive(&sink, &config, opts).await.unwrap();

    assert!(report.is_dry_run);
    assert_eq!(report.files_deleted, 0);
    assert_eq!(report.empty_directories_removed, 0);
    assert_eq!(report.empty_directories_eligible, 3); // holiday, 15-Dec-2022, 2022

    // Everything must remain physically present on disk in dry run:
    assert!(dup_file.exists());
    assert!(nested_trip.exists());
    assert!(unorganized_photos.join("2022").exists());
}

#[tokio::test]
async fn test_prune_all_drives_without_drive_parameter() {
    let tmp = tempdir().unwrap();
    let primary_dir = tmp.path().join("primary");
    let replica_dir = tmp.path().join("replica");
    let staging1_dir = tmp.path().join("staging1");
    let staging2_dir = tmp.path().join("staging2");

    std::fs::create_dir_all(&primary_dir).unwrap();
    std::fs::create_dir_all(&replica_dir).unwrap();
    std::fs::create_dir_all(&staging1_dir).unwrap();
    std::fs::create_dir_all(&staging2_dir).unwrap();

    let mut groups = HashMap::new();
    groups.insert("primary".to_string(), DriveGroupSpec { priority: 100, description: None });
    groups.insert("replica".to_string(), DriveGroupSpec { priority: 80, description: None });
    groups.insert("source1".to_string(), DriveGroupSpec { priority: 20, description: None });
    groups.insert("source2".to_string(), DriveGroupSpec { priority: 10, description: None });

    let config = DriveConfiguration {
        version: "1.0".to_string(),
        groups,
        drives: vec![
            DriveDefinition {
                id: "primary_main".to_string(),
                mount_path: primary_dir.clone(),
                master_path: primary_dir.join("media/master"),
                group: "primary".to_string(),
            },
            DriveDefinition {
                id: "replica_1".to_string(),
                mount_path: replica_dir.clone(),
                master_path: replica_dir.join("media/master"),
                group: "replica".to_string(),
            },
            DriveDefinition {
                id: "staging_1".to_string(),
                mount_path: staging1_dir.clone(),
                master_path: staging1_dir.clone(),
                group: "source1".to_string(),
            },
            DriveDefinition {
                id: "staging_2".to_string(),
                mount_path: staging2_dir.clone(),
                master_path: staging2_dir.clone(),
                group: "source2".to_string(),
            },
        ],
        rules: DriveRules::default(),
    };

    // Master file on primary
    let master_file = primary_dir.join("media/master/2023/01-Jan-2023/Photos/photo.jpg");
    std::fs::create_dir_all(master_file.parent().unwrap()).unwrap();
    std::fs::write(&master_file, b"canonical master bitstream content").unwrap();

    // Duplicate 1 on staging1
    let dup1_file = staging1_dir.join("dup1.jpg");
    std::fs::write(&dup1_file, b"canonical master bitstream content").unwrap();

    // Duplicate 2 on staging2
    let dup2_file = staging2_dir.join("subfolder/dup2.jpg");
    std::fs::create_dir_all(dup2_file.parent().unwrap()).unwrap();
    std::fs::write(&dup2_file, b"canonical master bitstream content").unwrap();

    let sink = MockPruneStorageSink::default();
    let service = DrivePruningService::new();

    let master_id = Uuid::new_v4();
    let master_doc = create_doc_with_dedup(
        master_id,
        master_file.clone(),
        35,
        DeduplicationStatus::CanonicalMaster,
        None,
        None,
    );

    let dup1_id = Uuid::new_v4();
    let dup1_doc = create_doc_with_dedup(
        dup1_id,
        dup1_file.clone(),
        35,
        DeduplicationStatus::MarkForRemoval,
        Some(master_id),
        Some(master_file.clone()),
    );

    let dup2_id = Uuid::new_v4();
    let dup2_doc = create_doc_with_dedup(
        dup2_id,
        dup2_file.clone(),
        35,
        DeduplicationStatus::MarkForRemoval,
        Some(master_id),
        Some(master_file.clone()),
    );

    sink.save_batch(&[master_doc, dup1_doc, dup2_doc]).await.unwrap();

    // 1. Dry run across all catalog drives (drive_identifier: None)
    let dry_opts = DrivePrunerOptions::for_all_drives(false);
    let dry_report = service.prune_drive(&sink, &config, dry_opts).await.unwrap();

    assert!(dry_report.is_dry_run);
    assert_eq!(dry_report.drive_id, "all");
    assert_eq!(dry_report.total_candidates, 2);
    assert_eq!(dry_report.verified_masters, 2);
    assert_eq!(dry_report.files_deleted, 0);
    assert!(dup1_file.exists());
    assert!(dup2_file.exists());

    // 2. Execute prune across all catalog drives (drive_identifier: None)
    let mut exec_opts = DrivePrunerOptions::for_all_drives(true);
    exec_opts.require_master_disk = true;
    let exec_report = service.prune_drive(&sink, &config, exec_opts).await.unwrap();

    assert!(!exec_report.is_dry_run);
    assert_eq!(exec_report.drive_id, "all");
    assert_eq!(exec_report.total_candidates, 2);
    assert_eq!(exec_report.files_deleted, 2);
    assert_eq!(exec_report.records_purged, 2);
    assert_eq!(exec_report.errors.len(), 0);

    // Files deleted from disk
    assert!(!dup1_file.exists(), "dup1 should be deleted from staging1");
    assert!(!dup2_file.exists(), "dup2 should be deleted from staging2");
    assert!(master_file.exists(), "master file must remain untouched");

    // Database records purged
    assert!(sink.find_document_by_id(dup1_id).await.unwrap().is_none());
    assert!(sink.find_document_by_id(dup2_id).await.unwrap().is_none());
    assert!(sink.find_document_by_id(master_id).await.unwrap().is_some());
}

#[tokio::test]
async fn test_prune_forbidden_on_primary_and_replica_extension_pools() {
    let temp_root = tempdir().unwrap();

    let primary_main = temp_root.path().join("primary_main");
    let primary_main_master = primary_main.join("SPSA").join("media").join("master");

    let primary_ext = temp_root.path().join("primary_ext");
    let primary_ext_master = primary_ext.join("SPSA").join("media").join("master");

    let replica_ext = temp_root.path().join("replica_ext");
    let replica_ext_master = replica_ext.join("SPSA").join("media").join("master");

    std::fs::create_dir_all(&primary_main_master).unwrap();
    std::fs::create_dir_all(&primary_ext_master).unwrap();
    std::fs::create_dir_all(&replica_ext_master).unwrap();

    let mut groups = HashMap::new();
    groups.insert(
        "primary".to_string(),
        DriveGroupSpec {
            priority: 100,
            description: Some("Primary Pool".to_string()),
        },
    );
    groups.insert(
        "replica1".to_string(),
        DriveGroupSpec {
            priority: 80,
            description: Some("Replica 1 Pool".to_string()),
        },
    );

    let config = DriveConfiguration {
        version: "1.0".to_string(),
        groups,
        drives: vec![
            DriveDefinition {
                id: "primary_main".to_string(),
                mount_path: primary_main.clone(),
                master_path: primary_main_master.clone(),
                group: "primary".to_string(),
            },
            DriveDefinition {
                id: "primary_ext_1".to_string(),
                mount_path: primary_ext.clone(),
                master_path: primary_ext_master.clone(),
                group: "primary".to_string(),
            },
            DriveDefinition {
                id: "replica_1_ext_1".to_string(),
                mount_path: replica_ext.clone(),
                master_path: replica_ext_master.clone(),
                group: "replica1".to_string(),
            },
        ],
        rules: DriveRules::default(),
    };

    let sink = MockPruneStorageSink::default();
    let service = DrivePruningService::new();

    // 1. Attempt prune targeting primary extension master directory
    let res_pri_ext = service
        .prune_drive(
            &sink,
            &config,
            DrivePrunerOptions::for_drive(primary_ext_master.to_string_lossy(), false),
        )
        .await;

    assert!(
        res_pri_ext.is_err(),
        "Pruning primary extension master must be rejected by archive protection"
    );
    match res_pri_ext.unwrap_err() {
        metadata_extractor::domain::errors::DeduplicationError::PruneForbiddenOnProtectedArchive { .. } => {}
        other => panic!("Expected PruneForbiddenOnProtectedArchive, got {:?}", other),
    }

    // 2. Attempt prune targeting replica extension master directory
    let res_rep_ext = service
        .prune_drive(
            &sink,
            &config,
            DrivePrunerOptions::for_drive(replica_ext_master.to_string_lossy(), false),
        )
        .await;

    assert!(
        res_rep_ext.is_err(),
        "Pruning replica extension master must be rejected by archive protection"
    );
    match res_rep_ext.unwrap_err() {
        metadata_extractor::domain::errors::DeduplicationError::PruneForbiddenOnProtectedArchive { .. } => {}
        other => panic!("Expected PruneForbiddenOnProtectedArchive, got {:?}", other),
    }

    // 3. Attempt safe_delete_file directly via ArchiveProtectionGuard
    let guard = metadata_extractor::infrastructure::archive_guard::ArchiveProtectionGuard::from_config(&config);
    let pri_ext_file = primary_ext_master.join("2026/01-Jan-2026/Photos/img.jpg");
    assert!(guard.is_protected_path(&pri_ext_file));
    assert!(guard.safe_delete_file(&pri_ext_file).is_err());

    let rep_ext_file = replica_ext_master.join("2026/01-Jan-2026/Photos/img.jpg");
    assert!(guard.is_protected_path(&rep_ext_file));
    assert!(guard.safe_delete_file(&rep_ext_file).is_err());
}
