use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use uuid::Uuid;

use metadata_extractor::application::ingest_service::MasterIngestService;
use metadata_extractor::domain::dedup_models::{
    DeduplicationMetadata, DeduplicationStatus, DriveConfiguration, DriveDefinition,
    DriveGroupSpec, DriveRules, IngestOptions, IngestTaskStatus, MasterReplicationState,
    MediaFingerprint, ReplicationStatus,
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
struct MockIngestStorageSink {
    documents: Arc<Mutex<HashMap<Uuid, MediaMetadataDocument>>>,
    marked_removed: Arc<Mutex<Vec<Uuid>>>,
}

#[async_trait::async_trait]
impl MetadataStorageSink for MockIngestStorageSink {
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

    async fn link_canonical_masters(
        &self,
        links: &[(String, Uuid, PathBuf)],
    ) -> Result<usize, StorageError> {
        let mut guard = self.documents.lock().unwrap();
        let mut count = 0;
        for (hash, master_id, master_path) in links {
            for doc in guard.values_mut() {
                if let Some(ref fp) = doc.fingerprint {
                    if &fp.cryptographic_hash == hash {
                        let is_master = doc
                            .dedup
                            .as_ref()
                            .map(|d| d.status == DeduplicationStatus::CanonicalMaster)
                            .unwrap_or(false);
                        if !is_master {
                            if let Some(ref mut d) = doc.dedup {
                                if d.status == DeduplicationStatus::NeedsIngestToMaster {
                                    d.status = DeduplicationStatus::MarkForRemoval;
                                }
                                d.canonical_master_id = Some(*master_id);
                                d.canonical_master_path = Some(master_path.clone());
                            }
                            count += 1;
                        }
                    }
                }
            }
        }
        Ok(count)
    }

    async fn update_master_replication_state(
        &self,
        master_id: Uuid,
        replication_state: &MasterReplicationState,
    ) -> Result<(), StorageError> {
        let mut guard = self.documents.lock().unwrap();
        if let Some(doc) = guard.get_mut(&master_id) {
            if let Some(ref mut d) = doc.dedup {
                d.replication = Some(replication_state.clone());
            }
        }
        Ok(())
    }

    async fn query_masters_by_replication_status(
        &self,
        status: ReplicationStatus,
    ) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        let guard = self.documents.lock().unwrap();
        let matches = guard
            .values()
            .filter(|d| {
                d.dedup
                    .as_ref()
                    .and_then(|meta| meta.replication.as_ref())
                    .map(|r| r.status == status)
                    .unwrap_or(false)
            })
            .cloned()
            .collect();
        Ok(matches)
    }
}

fn create_unmastered_doc(
    id: Uuid,
    path: PathBuf,
    size: u64,
    cat: MediaCategory,
    dt: DateTime<Utc>,
    hash: String,
) -> MediaMetadataDocument {
    let filename = path.file_name().unwrap().to_string_lossy().to_string();

    MediaMetadataDocument {
        id,
        file_context: FileContext {
            scanned_root: PathBuf::from("/"),
            file_path: path.clone(),
            relative_path: PathBuf::from(&filename),
            file_name: filename,
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
            file_path: path,
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
            status: DeduplicationStatus::NeedsIngestToMaster,
            canonical_master_id: None,
            canonical_master_path: None,
            drive_id: Some("source_dump".to_string()),
            drive_group: Some("scratch".to_string()),
            is_in_master_path: false,
            reclaimable_bytes: 0,
            evaluated_at: dt,
            replication: None,
        }),
        visual: None,
    }
}

#[tokio::test]
async fn test_ingest_plan_compilation_and_dry_run() {
    let temp_root = tempdir().unwrap();
    let source_mount = temp_root.path().join("DumpDrive");
    let target_mount = temp_root.path().join("PrimaryDrive");
    let target_master = target_mount.join("SPSA/media/master");

    std::fs::create_dir_all(&source_mount).unwrap();
    std::fs::create_dir_all(&target_master).unwrap();

    let mut groups = HashMap::new();
    groups.insert(
        "primary".to_string(),
        DriveGroupSpec {
            priority: 100,
            description: None,
        },
    );
    groups.insert(
        "scratch".to_string(),
        DriveGroupSpec {
            priority: 40,
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
                mount_path: target_mount.clone(),
                master_path: target_master.clone(),
            },
            DriveDefinition {
                id: "source_dump".to_string(),
                group: "scratch".to_string(),
                mount_path: source_mount.clone(),
                master_path: source_mount.join("master"),
            },
        ],
        rules: DriveRules::default(),
    };

    let sink = MockIngestStorageSink::default();

    // Create a dummy photo file on the source drive
    let src_file = source_mount.join("DCIM/IMG_1042.JPG");
    std::fs::create_dir_all(src_file.parent().unwrap()).unwrap();
    std::fs::write(&src_file, b"test photo content for dry run").unwrap();

    let hasher = Blake3MediaHasher::new();
    use metadata_extractor::domain::ports::MediaFingerprinter;
    let hash = hasher.compute_cryptographic_hash(&src_file).unwrap();

    let dt = "2021-11-30T10:00:00Z".parse::<DateTime<Utc>>().unwrap();
    let doc = create_unmastered_doc(
        Uuid::new_v4(),
        src_file.clone(),
        src_file.metadata().unwrap().len(),
        MediaCategory::Photo,
        dt,
        hash,
    );
    sink.save_batch(&[doc]).await.unwrap();

    let service = MasterIngestService::new();
    let options = IngestOptions {
        source_drive: "source_dump".to_string(),
        target_drive: Some("primary_main".to_string()),
        dry_run: true,
        delete_source: false,
        verify_hash: true,
        buffer_config: Default::default(),
    };

    let plan = service
        .compile_plan(&sink, &config, &options)
        .await
        .unwrap();
    assert_eq!(plan.tasks.len(), 1);
    assert_eq!(plan.tasks[0].status, IngestTaskStatus::Pending);

    // Verify canonical path conforms to <MasterRoot>/<Year>/<DD-Mon-YYYY>/Photos/IMG_1042.JPG
    let expected_target = target_master.join("2021/30-Nov-2021/Photos/IMG_1042.JPG");
    assert_eq!(plan.tasks[0].target_path, expected_target);

    // Run execution with dry-run
    let report = service
        .execute_ingest(&sink, &config, options)
        .await
        .unwrap();
    assert!(report.is_dry_run);
    assert_eq!(report.files_transferred, 0);
    // Destination file must NOT exist!
    assert!(!expected_target.exists());
}

#[tokio::test]
async fn test_ingest_execution_with_integrity_and_collision_handling() {
    let temp_root = tempdir().unwrap();
    let source_mount = temp_root.path().join("DumpDrive");
    let target_mount = temp_root.path().join("PrimaryDrive");
    let target_master = target_mount.join("SPSA/media/master");

    std::fs::create_dir_all(&source_mount).unwrap();
    std::fs::create_dir_all(&target_master).unwrap();

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
                mount_path: target_mount.clone(),
                master_path: target_master.clone(),
            },
            DriveDefinition {
                id: "source_dump".to_string(),
                group: "staging".to_string(),
                mount_path: source_mount.clone(),
                master_path: source_mount.join("master"),
            },
        ],
        rules: DriveRules::default(),
    };

    let sink = MockIngestStorageSink::default();

    // 1. Create source photo file
    let src_file = source_mount.join("camera/IMG_0001.JPG");
    std::fs::create_dir_all(src_file.parent().unwrap()).unwrap();
    std::fs::write(&src_file, b"awesome new photo content").unwrap();

    let hasher = Blake3MediaHasher::new();
    use metadata_extractor::domain::ports::MediaFingerprinter;
    let hash = hasher.compute_cryptographic_hash(&src_file).unwrap();

    let dt = "2021-11-30T10:00:00Z".parse::<DateTime<Utc>>().unwrap();
    let src_doc_id = Uuid::new_v4();
    let doc = create_unmastered_doc(
        src_doc_id,
        src_file.clone(),
        src_file.metadata().unwrap().len(),
        MediaCategory::Photo,
        dt,
        hash,
    );
    sink.save_batch(&[doc]).await.unwrap();

    // 2. Pre-create a colliding file at destination with DIFFERENT content
    let existing_dest = target_master.join("2021/30-Nov-2021/Photos/IMG_0001.JPG");
    std::fs::create_dir_all(existing_dest.parent().unwrap()).unwrap();
    std::fs::write(&existing_dest, b"different older photo content").unwrap();

    let service = MasterIngestService::new();
    let options = IngestOptions {
        source_drive: "source_dump".to_string(),
        target_drive: Some("primary_main".to_string()),
        dry_run: false,
        delete_source: true, // test immediate deletion
        verify_hash: true,
        buffer_config: Default::default(),
    };

    let report = service
        .execute_ingest(&sink, &config, options)
        .await
        .unwrap();
    assert!(!report.is_dry_run);
    assert_eq!(report.files_transferred, 1);
    assert_eq!(report.files_failed, 0);
    assert_eq!(report.source_files_deleted, 1);

    // Old colliding file must remain untouched
    assert!(existing_dest.exists());
    assert_eq!(
        std::fs::read(&existing_dest).unwrap(),
        b"different older photo content"
    );

    // New ingested file must be saved under non-colliding name IMG_0001_1.JPG
    let resolved_dest = target_master.join("2021/30-Nov-2021/Photos/IMG_0001_1.JPG");
    assert!(resolved_dest.exists());
    assert_eq!(
        std::fs::read(&resolved_dest).unwrap(),
        b"awesome new photo content"
    );

    // Source file must be deleted from disk
    assert!(!src_file.exists());

    // Check DB records:
    // Source document status should be Removed
    let src_doc = sink.find_document_by_id(src_doc_id).await.unwrap().unwrap();
    assert_eq!(src_doc.dedup.unwrap().status, DeduplicationStatus::Removed);

    // New master document must exist with status CanonicalMaster
    let docs = sink.documents.lock().unwrap().clone();
    let master_doc = docs
        .values()
        .find(|d| d.file_context.file_path == resolved_dest)
        .unwrap();
    assert_eq!(
        master_doc.dedup.as_ref().unwrap().status,
        DeduplicationStatus::CanonicalMaster
    );
    assert!(master_doc.dedup.as_ref().unwrap().is_in_master_path);
}

#[tokio::test]
async fn test_ingest_integrity_mismatch_rollback() {
    let temp_root = tempdir().unwrap();
    let source_mount = temp_root.path().join("DumpDrive");
    let target_mount = temp_root.path().join("PrimaryDrive");
    let target_master = target_mount.join("SPSA/media/master");

    std::fs::create_dir_all(&source_mount).unwrap();
    std::fs::create_dir_all(&target_master).unwrap();

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
                mount_path: target_mount.clone(),
                master_path: target_master.clone(),
            },
            DriveDefinition {
                id: "source_dump".to_string(),
                group: "staging".to_string(),
                mount_path: source_mount.clone(),
                master_path: source_mount.join("master"),
            },
        ],
        rules: DriveRules::default(),
    };

    let sink = MockIngestStorageSink::default();

    let src_file = source_mount.join("corrupt_test.jpg");
    std::fs::write(&src_file, b"content on disk").unwrap();

    let dt = "2021-11-30T10:00:00Z".parse::<DateTime<Utc>>().unwrap();
    let src_doc_id = Uuid::new_v4();
    // Record with an expected hash that DOES NOT match the file content
    let doc = create_unmastered_doc(
        src_doc_id,
        src_file.clone(),
        src_file.metadata().unwrap().len(),
        MediaCategory::Photo,
        dt,
        "bogus_hash_mismatch".to_string(),
    );
    sink.save_batch(&[doc]).await.unwrap();

    let service = MasterIngestService::new();
    let options = IngestOptions {
        source_drive: "source_dump".to_string(),
        target_drive: Some("primary_main".to_string()),
        dry_run: false,
        delete_source: false,
        verify_hash: true,
        buffer_config: Default::default(),
    };

    let report = service
        .execute_ingest(&sink, &config, options)
        .await
        .unwrap();
    assert_eq!(report.files_failed, 1);
    assert_eq!(report.files_transferred, 0);

    // Destination file must be deleted upon hash mismatch
    let expected_target = target_master.join("2021/30-Nov-2021/Photos/corrupt_test.jpg");
    assert!(!expected_target.exists());

    // Source file must remain completely intact!
    assert!(src_file.exists());

    // Source document status must still be NeedsIngestToMaster
    let src_doc = sink.find_document_by_id(src_doc_id).await.unwrap().unwrap();
    assert_eq!(
        src_doc.dedup.unwrap().status,
        DeduplicationStatus::NeedsIngestToMaster
    );
}

#[tokio::test]
async fn test_ingest_source_on_replica_transitions_to_replica_keep() {
    let temp_root = tempdir().unwrap();
    let replica_mount = temp_root.path().join("ReplicaDrive");
    let replica_master = replica_mount.join("SPSA/media/master");
    let target_mount = temp_root.path().join("PrimaryDrive");
    let target_master = target_mount.join("SPSA/media/master");

    std::fs::create_dir_all(&replica_master).unwrap();
    std::fs::create_dir_all(&target_master).unwrap();

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

    let config = DriveConfiguration {
        version: "1.0".to_string(),
        groups,
        drives: vec![
            DriveDefinition {
                id: "primary_main".to_string(),
                group: "primary".to_string(),
                mount_path: target_mount.clone(),
                master_path: target_master.clone(),
            },
            DriveDefinition {
                id: "replica_1".to_string(),
                group: "replica1".to_string(),
                mount_path: replica_mount.clone(),
                master_path: replica_master.clone(),
            },
        ],
        rules: DriveRules::default(),
    };

    // Create file inside replica's canonical master path: 2021/30-Nov-2021/Photos/photo.jpg
    let src_dir = replica_master.join("2021/30-Nov-2021/Photos");
    std::fs::create_dir_all(&src_dir).unwrap();
    let src_file = src_dir.join("photo.jpg");
    std::fs::write(&src_file, b"Replica source unique content").unwrap();

    let hasher = Blake3MediaHasher::new();
    let hash = hasher.compute_cryptographic_hash(&src_file).unwrap();

    let sink = MockIngestStorageSink::default();
    let src_doc_id = Uuid::new_v4();
    let dt = DateTime::parse_from_rfc3339("2021-11-30T10:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    let doc = create_unmastered_doc(
        src_doc_id,
        src_file.clone(),
        src_file.metadata().unwrap().len(),
        MediaCategory::Photo,
        dt,
        hash,
    );
    sink.save_batch(&[doc]).await.unwrap();

    let service = MasterIngestService::new();
    let options = IngestOptions {
        source_drive: "replica_1".to_string(),
        target_drive: Some("primary_main".to_string()),
        dry_run: false,
        delete_source: true, // Even if delete_source is set, replica copy must NOT be deleted
        verify_hash: true,
        buffer_config: Default::default(),
    };

    let report = service
        .execute_ingest(&sink, &config, options)
        .await
        .unwrap();

    assert_eq!(report.files_transferred, 1);
    assert_eq!(report.source_files_deleted, 0); // Preserved!
    assert!(src_file.exists()); // Source on replica remains!

    // Source doc must transition to ReplicaKeep!
    let src_doc = sink.find_document_by_id(src_doc_id).await.unwrap().unwrap();
    let src_meta = src_doc.dedup.unwrap();
    assert_eq!(src_meta.status, DeduplicationStatus::ReplicaKeep);
    assert!(src_meta.is_in_master_path);
    assert_eq!(src_meta.reclaimable_bytes, 0);
}

#[tokio::test]
async fn test_ingest_and_replicate_fanout_end_to_end() {
    let temp_root = tempdir().unwrap();
    let staging_mount = temp_root.path().join("StagingDrive");
    let primary_mount = temp_root.path().join("PrimaryDrive");
    let primary_master = primary_mount.join("SPSA/media/master");
    let replica_mount = temp_root.path().join("ReplicaDrive");
    let replica_master = replica_mount.join("SPSA/media/master");

    std::fs::create_dir_all(&staging_mount).unwrap();
    std::fs::create_dir_all(&primary_master).unwrap();
    std::fs::create_dir_all(&replica_master).unwrap();

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
                id: "replica_1".to_string(),
                group: "replica1".to_string(),
                mount_path: replica_mount.clone(),
                master_path: replica_master.clone(),
            },
            DriveDefinition {
                id: "staging_drive".to_string(),
                group: "staging".to_string(),
                mount_path: staging_mount.clone(),
                master_path: staging_mount.join("master"),
            },
        ],
        rules: DriveRules::default(),
    };

    let src_file = staging_mount.join("new_photo.jpg");
    std::fs::write(
        &src_file,
        b"Unique photo from digital camera on staging drive",
    )
    .unwrap();

    let hasher = Blake3MediaHasher::new();
    let hash = hasher.compute_cryptographic_hash(&src_file).unwrap();

    let sink = MockIngestStorageSink::default();
    let src_doc_id = Uuid::new_v4();
    let dt = DateTime::parse_from_rfc3339("2024-05-10T14:30:00Z")
        .unwrap()
        .with_timezone(&Utc);

    let doc = create_unmastered_doc(
        src_doc_id,
        src_file.clone(),
        src_file.metadata().unwrap().len(),
        MediaCategory::Photo,
        dt,
        hash.clone(),
    );
    sink.save_batch(&[doc]).await.unwrap();

    // 1. Execute Master Ingest
    let ingest_service = MasterIngestService::new();
    let ingest_options = IngestOptions {
        source_drive: "staging_drive".to_string(),
        target_drive: Some("primary_main".to_string()),
        dry_run: false,
        delete_source: false,
        verify_hash: true,
        buffer_config: Default::default(),
    };

    let ingest_report = ingest_service
        .execute_ingest(&sink, &config, ingest_options)
        .await
        .unwrap();

    assert_eq!(ingest_report.files_transferred, 1);
    assert_eq!(ingest_report.files_failed, 0);

    // Primary master file must exist
    let primary_file = primary_master.join("2024/10-May-2024/Photos/new_photo.jpg");
    assert!(primary_file.exists());
    assert_eq!(
        hasher.compute_cryptographic_hash(&primary_file).unwrap(),
        hash
    );

    // 2. Execute Replica Fanout
    let rep_service = metadata_extractor::application::replica_service::ReplicaSyncService::new();
    let rep_options = metadata_extractor::application::replica_service::ReplicaSyncOptions {
        target_replica_id: None,
        primary_drive_id: Some("primary_main".to_string()),
        dry_run: false,
        verify_hash: true,
        buffer_config: Default::default(),
    };

    let rep_report = rep_service
        .execute_sync(&sink, &config, rep_options)
        .await
        .unwrap();

    assert_eq!(rep_report.files_replicated, 1);
    assert_eq!(rep_report.files_failed, 0);

    // Replica file must exist with identical content and hash
    let replica_file = replica_master.join("2024/10-May-2024/Photos/new_photo.jpg");
    assert!(replica_file.exists());
    assert_eq!(
        hasher.compute_cryptographic_hash(&replica_file).unwrap(),
        hash
    );

    // Catalog state verification
    let rep_docs = sink.query_replica_documents("replica_1").await.unwrap();
    assert_eq!(rep_docs.len(), 1);
    assert_eq!(
        rep_docs[0].dedup.as_ref().unwrap().status,
        DeduplicationStatus::ReplicaKeep
    );
}

#[tokio::test]
async fn test_ingest_source_drive_unconfigured_external_or_local_directory() {
    let temp_root = tempdir().unwrap();

    let primary_mount = temp_root.path().join("primary_drive");
    let primary_master = primary_mount.join("master");
    std::fs::create_dir_all(&primary_master).unwrap();

    // Source drive is an unconfigured external drive mount or local directory path
    let unconfigured_source = temp_root.path().join("external_sd_card/DCIM");
    std::fs::create_dir_all(&unconfigured_source).unwrap();

    let src_file = unconfigured_source.join("IMG_4242.JPG");
    std::fs::write(&src_file, b"unconfigured external drive photo content").unwrap();

    let hasher = Blake3MediaHasher::new();
    let hash = hasher.compute_cryptographic_hash(&src_file).unwrap();

    let dt = "2023-08-15T14:30:00Z".parse::<DateTime<Utc>>().unwrap();
    let doc = create_unmastered_doc(
        Uuid::new_v4(),
        src_file.clone(),
        src_file.metadata().unwrap().len(),
        MediaCategory::Photo,
        dt,
        hash.clone(),
    );

    let sink = MockIngestStorageSink::default();
    sink.save_batch(&[doc]).await.unwrap();

    // Configuration ONLY contains the primary master drive. The source drive is NOT configured anywhere!
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
            id: "primary_main".to_string(),
            group: "primary".to_string(),
            mount_path: primary_mount.clone(),
            master_path: primary_master.clone(),
        }],
        rules: DriveRules::default(),
    };

    let service = MasterIngestService::new();

    // 1. Plan compilation using raw directory path
    let options = IngestOptions {
        source_drive: unconfigured_source.to_string_lossy().to_string(),
        target_drive: None,
        dry_run: true,
        delete_source: false,
        verify_hash: true,
        buffer_config: Default::default(),
    };

    let plan = service
        .compile_plan(&sink, &config, &options)
        .await
        .expect("compile_plan should succeed for unconfigured source directory");

    assert_eq!(plan.source_drive_id, "DCIM");
    assert_eq!(plan.source_mount, unconfigured_source);
    assert_eq!(plan.target_drive_id, "primary_main");
    assert_eq!(plan.tasks.len(), 1);

    let expected_target = primary_master.join("2023/15-Aug-2023/Photos/IMG_4242.JPG");
    assert_eq!(plan.tasks[0].target_path, expected_target);

    // 2. Ingest execution
    let execute_options = IngestOptions {
        source_drive: unconfigured_source.to_string_lossy().to_string(),
        target_drive: None,
        dry_run: false,
        delete_source: false,
        verify_hash: true,
        buffer_config: Default::default(),
    };

    let report = service
        .execute_ingest(&sink, &config, execute_options)
        .await
        .expect("execute_ingest should succeed for unconfigured source directory");

    assert_eq!(report.files_transferred, 1);
    assert_eq!(report.files_failed, 0);
    assert_eq!(report.source_drive_id, "DCIM");
    assert!(expected_target.exists());
    assert_eq!(
        hasher.compute_cryptographic_hash(&expected_target).unwrap(),
        hash
    );

    // 3. Test non-existent path errors cleanly with DriveNotMounted
    let bad_options = IngestOptions {
        source_drive: "/tmp/nonexistent_path_xyz_12345".to_string(),
        target_drive: None,
        dry_run: true,
        delete_source: false,
        verify_hash: true,
        buffer_config: Default::default(),
    };
    let err = service
        .compile_plan(&sink, &config, &bad_options)
        .await
        .unwrap_err();
    match err {
        metadata_extractor::domain::errors::DeduplicationError::DriveNotMounted { .. } => {}
        other => panic!("Expected DriveNotMounted, got: {:?}", other),
    }
}

#[tokio::test]
async fn test_ingest_updates_canonical_master_id_for_all_duplicate_records() {
    let source_dir = tempdir().unwrap();
    let master_dir = tempdir().unwrap();

    let content = b"cluster of multiple identical files content test";
    let file1_path = source_dir.path().join("lv_0_20260724194130 copy 3.mp4");
    let file2_path = source_dir.path().join("lv_0_20260724194130 copy 2.mp4");
    let file3_path = source_dir.path().join("lv_0_20260724194130.mp4");

    std::fs::write(&file1_path, content).unwrap();
    std::fs::write(&file2_path, content).unwrap();
    std::fs::write(&file3_path, content).unwrap();

    let hasher = Blake3MediaHasher::new();
    let hash = hasher.compute_cryptographic_hash(&file1_path).unwrap();

    let sink = MockIngestStorageSink::default();
    let now = Utc::now();
    let cluster_id = Uuid::new_v4();

    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let id3 = Uuid::new_v4();

    let make_doc = |id: Uuid, path: PathBuf, status: DeduplicationStatus| MediaMetadataDocument {
        id,
        file_context: FileContext {
            scanned_root: source_dir.path().to_path_buf(),
            file_path: path.clone(),
            relative_path: path.clone(),
            file_name: path.file_name().unwrap().to_string_lossy().to_string(),
            file_size_bytes: content.len() as u64,
            extension: "mp4".to_string(),
            media_category: MediaCategory::Video,
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
            file_path: path.clone(),
            file_size_bytes: content.len() as u64,
            media_category: MediaCategory::Video,
            dimensions: None,
            sample_digest: None,
            cryptographic_hash: hash.clone(),
            perceptual_hash: None,
            phash_hex: None,
            rotated_hashes: None,
            is_low_entropy: false,
            fingerprinted_at: now,
        }),
        extraction_status: ExtractionStatus::default(),
        dedup: Some(DeduplicationMetadata {
            cluster_id,
            status,
            canonical_master_id: None,
            canonical_master_path: None,
            drive_id: Some("samples".to_string()),
            drive_group: None,
            is_in_master_path: false,
            reclaimable_bytes: if status == DeduplicationStatus::MarkForRemoval {
                content.len() as u64
            } else {
                0
            },
            evaluated_at: now,
            replication: None,
        }),
        visual: None,
    };

    let doc1 = make_doc(
        id1,
        file1_path.clone(),
        DeduplicationStatus::NeedsIngestToMaster,
    );
    let doc2 = make_doc(id2, file2_path.clone(), DeduplicationStatus::MarkForRemoval);
    let doc3 = make_doc(id3, file3_path.clone(), DeduplicationStatus::MarkForRemoval);

    sink.save_batch(&[doc1, doc2, doc3]).await.unwrap();

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
            priority: 10,
            description: None,
        },
    );

    let config = DriveConfiguration {
        version: "1.0".to_string(),
        groups,
        drives: vec![
            DriveDefinition {
                id: "samples".to_string(),
                mount_path: source_dir.path().to_path_buf(),
                master_path: source_dir.path().to_path_buf(),
                group: "staging".to_string(),
            },
            DriveDefinition {
                id: "primary_drive".to_string(),
                mount_path: master_dir.path().to_path_buf(),
                master_path: master_dir.path().join("master"),
                group: "primary".to_string(),
            },
        ],
        rules: DriveRules::default(),
    };

    let service = MasterIngestService::new();
    let options = IngestOptions {
        source_drive: source_dir.path().to_string_lossy().to_string(),
        target_drive: None,
        dry_run: false,
        delete_source: false,
        verify_hash: true,
        buffer_config: Default::default(),
    };

    let report = service
        .execute_ingest(&sink, &config, options)
        .await
        .unwrap();

    assert_eq!(report.files_transferred, 1);
    assert_eq!(report.files_failed, 0);

    // Verify database state:
    // 1. One canonical master was saved with a new ID
    let masters = sink.query_canonical_masters().await.unwrap();
    assert_eq!(masters.len(), 1);
    let master_doc = &masters[0];
    let new_master_id = master_doc.id;
    assert_ne!(new_master_id, id1);
    assert_ne!(new_master_id, id2);
    assert_ne!(new_master_id, id3);
    assert!(master_doc.fingerprint.is_some());
    assert_eq!(
        master_doc.fingerprint.as_ref().unwrap().cryptographic_hash,
        hash
    );

    // 2. Transferred source document (doc1) has canonical_master_id set
    let updated_doc1 = sink.find_document_by_id(id1).await.unwrap().unwrap();
    let dedup1 = updated_doc1.dedup.unwrap();
    assert_eq!(dedup1.status, DeduplicationStatus::MarkForRemoval);
    assert_eq!(dedup1.canonical_master_id, Some(new_master_id));
    assert_eq!(
        dedup1.canonical_master_path,
        Some(master_doc.file_context.file_path.clone())
    );

    // 3. Sibling duplicate documents (doc2 and doc3) ALSO have canonical_master_id and canonical_master_path set!
    let updated_doc2 = sink.find_document_by_id(id2).await.unwrap().unwrap();
    let dedup2 = updated_doc2.dedup.unwrap();
    assert_eq!(dedup2.status, DeduplicationStatus::MarkForRemoval);
    assert_eq!(dedup2.canonical_master_id, Some(new_master_id));
    assert_eq!(
        dedup2.canonical_master_path,
        Some(master_doc.file_context.file_path.clone())
    );

    let updated_doc3 = sink.find_document_by_id(id3).await.unwrap().unwrap();
    let dedup3 = updated_doc3.dedup.unwrap();
    assert_eq!(dedup3.status, DeduplicationStatus::MarkForRemoval);
    assert_eq!(dedup3.canonical_master_id, Some(new_master_id));
    assert_eq!(
        dedup3.canonical_master_path,
        Some(master_doc.file_context.file_path.clone())
    );
}

#[tokio::test]
async fn test_ingest_5_copies_cluster_registers_master_and_links_all_duplicates_and_initializes_replication(
) {
    let source_dir = tempdir().unwrap();
    let master_dir = tempdir().unwrap();
    let replica_dir = tempdir().unwrap();

    let content = b"cluster of 5 identical files test content";
    let file_names = [
        "lv_0_20260724194130.mp4",
        "lv_0_20260724194130 copy.mp4",
        "lv_0_20260724194130 copy 2.mp4",
        "lv_0_20260724194130 copy 3.mp4",
        "lv_0_20260724194130 copy 4.mp4",
    ];

    let mut file_paths = Vec::new();
    for name in &file_names {
        let path = source_dir.path().join(name);
        std::fs::write(&path, content).unwrap();
        file_paths.push(path);
    }

    let hasher = Blake3MediaHasher::new();
    let hash = hasher.compute_cryptographic_hash(&file_paths[0]).unwrap();

    let sink = MockIngestStorageSink::default();
    let now = Utc::now();
    let cluster_id = Uuid::new_v4();

    let mut doc_ids = Vec::new();
    let mut docs = Vec::new();

    // In a dedup pass, 1 is marked NeedsIngestToMaster (best candidate), and the other 4 are MarkForRemoval
    for (idx, path) in file_paths.iter().enumerate() {
        let id = Uuid::new_v4();
        doc_ids.push(id);
        let status = if idx == 3 {
            // Pick copy 3 as best candidate (mirroring user case)
            DeduplicationStatus::NeedsIngestToMaster
        } else {
            DeduplicationStatus::MarkForRemoval
        };

        docs.push(MediaMetadataDocument {
            id,
            file_context: FileContext {
                scanned_root: source_dir.path().to_path_buf(),
                file_path: path.clone(),
                relative_path: path.clone(),
                file_name: path.file_name().unwrap().to_string_lossy().to_string(),
                file_size_bytes: content.len() as u64,
                extension: "mp4".to_string(),
                media_category: MediaCategory::Video,
                modified_at: Some(now),
            },
            temporal: TemporalMetadata::from_utc_datetime(
                now,
                TimestampSource::ExifDateTimeOriginal,
            ),
            camera: None,
            exposure: None,
            location: None,
            dimensions: None,
            video: None,
            fingerprint: Some(MediaFingerprint {
                id: Uuid::new_v4(),
                file_path: path.clone(),
                file_size_bytes: content.len() as u64,
                media_category: MediaCategory::Video,
                dimensions: None,
                sample_digest: None,
                cryptographic_hash: hash.clone(),
                perceptual_hash: None,
                phash_hex: None,
                rotated_hashes: None,
                is_low_entropy: false,
                fingerprinted_at: now,
            }),
            extraction_status: ExtractionStatus::default(),
            dedup: Some(DeduplicationMetadata {
                cluster_id,
                status,
                canonical_master_id: None,
                canonical_master_path: None,
                drive_id: Some("samples".to_string()),
                drive_group: None,
                is_in_master_path: false,
                reclaimable_bytes: if status == DeduplicationStatus::MarkForRemoval {
                    content.len() as u64
                } else {
                    0
                },
                evaluated_at: now,
                replication: None,
            }),
            visual: None,
        });
    }

    sink.save_batch(&docs).await.unwrap();

    let mut groups = HashMap::new();
    groups.insert(
        "primary".to_string(),
        DriveGroupSpec {
            priority: 100,
            description: None,
        },
    );
    groups.insert(
        "replica".to_string(),
        DriveGroupSpec {
            priority: 50,
            description: None,
        },
    );

    let config = DriveConfiguration {
        version: "1.0".to_string(),
        groups,
        drives: vec![
            DriveDefinition {
                id: "samples".to_string(),
                mount_path: source_dir.path().to_path_buf(),
                master_path: source_dir.path().to_path_buf(),
                group: "staging".to_string(),
            },
            DriveDefinition {
                id: "primary_main".to_string(),
                mount_path: master_dir.path().to_path_buf(),
                master_path: master_dir.path().join("master"),
                group: "primary".to_string(),
            },
            DriveDefinition {
                id: "replica1".to_string(),
                mount_path: replica_dir.path().to_path_buf(),
                master_path: replica_dir.path().join("master"),
                group: "replica".to_string(),
            },
        ],
        rules: DriveRules::default(),
    };

    let service = MasterIngestService::new();
    let options = IngestOptions {
        source_drive: source_dir.path().to_string_lossy().to_string(),
        target_drive: None,
        dry_run: false,
        delete_source: false,
        verify_hash: true,
        buffer_config: Default::default(),
    };

    let report = service
        .execute_ingest(&sink, &config, options)
        .await
        .unwrap();

    // Verify Ingestion Report metrics
    assert_eq!(report.files_transferred, 1);
    assert_eq!(report.files_failed, 0);
    assert_eq!(report.duplicates_linked, 5); // All 5 copies linked!
    assert_eq!(report.replication_pending_count, 1); // 1 master asset pending replication to replica1

    // Verify CanonicalMaster in DB:
    let masters = sink.query_canonical_masters().await.unwrap();
    assert_eq!(masters.len(), 1);
    let master_doc = &masters[0];
    let new_master_id = master_doc.id;
    assert!(!doc_ids.contains(&new_master_id));
    assert!(master_doc.file_context.file_path.exists());
    assert_eq!(
        master_doc
            .file_context
            .file_path
            .parent()
            .unwrap()
            .file_name()
            .unwrap(),
        "Videos"
    );

    // Verify master replication status is initialized to Pending for replica1
    let master_rep = master_doc
        .dedup
        .as_ref()
        .unwrap()
        .replication
        .as_ref()
        .unwrap();
    assert_eq!(master_rep.status, ReplicationStatus::Pending);
    assert_eq!(master_rep.pending_drives, vec!["replica1".to_string()]);
    assert_eq!(master_rep.replicated_drives.len(), 0);

    // Verify ALL 5 copies in the cluster have canonical_master_id & canonical_master_path set
    for id in &doc_ids {
        let doc = sink.find_document_by_id(*id).await.unwrap().unwrap();
        let dedup = doc.dedup.unwrap();
        assert_eq!(dedup.status, DeduplicationStatus::MarkForRemoval);
        assert_eq!(dedup.canonical_master_id, Some(new_master_id));
        assert_eq!(
            dedup.canonical_master_path,
            Some(master_doc.file_context.file_path.clone())
        );
    }

    // Verify that DrivePruningService generates manifest for all 5 duplicates with zero warnings
    let pruner = metadata_extractor::application::drive_pruner::DrivePruningService::new();
    let manifest = pruner
        .generate_manifest(&sink, &config, Some("samples"), true)
        .await
        .unwrap();
    assert_eq!(manifest.total_files_to_remove, 5);
    assert_eq!(manifest.removal_tasks.len(), 5);
    for task in &manifest.removal_tasks {
        assert!(task.master_exists_verified);
    }
    assert_eq!(manifest.total_reclaimable_bytes, (content.len() * 5) as u64);
}

#[tokio::test]
async fn test_ingest_delete_source_never_deletes_protected_archive() {
    let source_dir = tempdir().unwrap();
    let replica_dir = tempdir().unwrap();
    let master_dir = tempdir().unwrap();

    let replica_file = replica_dir.path().join("Videos/precious_backup.mp4");
    std::fs::create_dir_all(replica_file.parent().unwrap()).unwrap();
    let content = b"irreplaceable video backup on replica";
    std::fs::write(&replica_file, content).unwrap();

    let hasher = Blake3MediaHasher::new();
    let hash = hasher.compute_cryptographic_hash(&replica_file).unwrap();

    let sink = MockIngestStorageSink::default();
    let replica_doc_id = Uuid::new_v4();
    let replica_doc = MediaMetadataDocument {
        id: replica_doc_id,
        file_context: FileContext {
            scanned_root: replica_dir.path().to_path_buf(),
            file_path: replica_file.clone(),
            relative_path: PathBuf::from("Videos/precious_backup.mp4"),
            file_name: "precious_backup.mp4".to_string(),
            file_size_bytes: content.len() as u64,
            extension: "mp4".to_string(),
            media_category: MediaCategory::Video,
            modified_at: Some(Utc::now()),
        },
        temporal: TemporalMetadata::from_utc_datetime(
            Utc::now(),
            TimestampSource::ExifDateTimeOriginal,
        ),
        camera: None,
        exposure: None,
        location: None,
        dimensions: None,
        video: None,
        fingerprint: Some(MediaFingerprint {
            id: Uuid::new_v4(),
            file_path: replica_file.clone(),
            file_size_bytes: content.len() as u64,
            media_category: MediaCategory::Video,
            dimensions: None,
            sample_digest: None,
            cryptographic_hash: hash.clone(),
            perceptual_hash: None,
            phash_hex: None,
            rotated_hashes: None,
            is_low_entropy: false,
            fingerprinted_at: Utc::now(),
        }),
        extraction_status: Default::default(),
        dedup: Some(DeduplicationMetadata {
            cluster_id: Uuid::new_v4(),
            status: DeduplicationStatus::NeedsIngestToMaster,
            canonical_master_id: None,
            canonical_master_path: None,
            drive_id: Some("replica_drive".to_string()),
            drive_group: Some("replica1".to_string()),
            is_in_master_path: false,
            reclaimable_bytes: 0,
            evaluated_at: Utc::now(),
            replication: None,
        }),
        visual: None,
    };
    sink.save_batch(&[replica_doc]).await.unwrap();

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

    let config = DriveConfiguration {
        version: "1.0".to_string(),
        groups,
        drives: vec![
            DriveDefinition {
                id: "replica_drive".to_string(),
                mount_path: replica_dir.path().to_path_buf(),
                master_path: replica_dir.path().to_path_buf(),
                group: "replica1".to_string(),
            },
            DriveDefinition {
                id: "primary_main".to_string(),
                mount_path: master_dir.path().to_path_buf(),
                master_path: master_dir.path().join("master"),
                group: "primary".to_string(),
            },
            DriveDefinition {
                id: "staging_drive".to_string(),
                mount_path: source_dir.path().to_path_buf(),
                master_path: source_dir.path().to_path_buf(),
                group: "staging".to_string(),
            },
        ],
        rules: DriveRules::default(),
    };

    let service = MasterIngestService::new();
    let opts = IngestOptions {
        source_drive: replica_dir.path().to_string_lossy().to_string(),
        target_drive: None,
        dry_run: false,
        delete_source: true, // Requested delete_source!
        verify_hash: true,
        buffer_config: Default::default(),
    };

    let report = service.execute_ingest(&sink, &config, opts).await.unwrap();

    // Source deletion on replica must be refused
    assert_eq!(report.files_transferred, 1);
    assert_eq!(report.source_files_deleted, 0);
    assert!(
        replica_file.exists(),
        "File on replica drive must NEVER be deleted!"
    );

    // Status in DB must not be Removed
    let updated_doc = sink
        .find_document_by_id(replica_doc_id)
        .await
        .unwrap()
        .unwrap();
    let status = updated_doc.dedup.unwrap().status;
    assert_ne!(status, DeduplicationStatus::Removed);
}

#[tokio::test]
async fn test_ingest_without_source_drive_ingests_all_unmastered_from_database() {
    let temp_primary = tempfile::tempdir().unwrap();
    let primary_mount = temp_primary.path().to_path_buf();
    let primary_master = primary_mount.join("master");
    std::fs::create_dir_all(&primary_master).unwrap();

    let temp_source = tempfile::tempdir().unwrap();
    let source_file = temp_source.path().join("unmastered_photo.jpg");
    std::fs::write(&source_file, b"unmastered photo payload").unwrap();

    let sink = MockIngestStorageSink::default();
    let doc_id = Uuid::new_v4();
    let unmastered_doc = MediaMetadataDocument {
        id: doc_id,
        file_context: FileContext {
            scanned_root: temp_source.path().to_path_buf(),
            file_path: source_file.clone(),
            relative_path: PathBuf::from("unmastered_photo.jpg"),
            file_name: "unmastered_photo.jpg".to_string(),
            file_size_bytes: 24,
            extension: "jpg".to_string(),
            media_category: MediaCategory::Photo,
            modified_at: Some(Utc::now()),
        },
        temporal: metadata_extractor::TemporalMetadata::from_utc_datetime(
            Utc::now(),
            metadata_extractor::TimestampSource::ExifDateTimeOriginal,
        ),
        camera: None,
        exposure: None,
        location: None,
        dimensions: None,
        video: None,
        fingerprint: None,
        extraction_status: Default::default(),
        dedup: Some(DeduplicationMetadata {
            cluster_id: Uuid::new_v4(),
            status: DeduplicationStatus::NeedsIngestToMaster,
            canonical_master_id: None,
            canonical_master_path: None,
            drive_id: None,
            drive_group: None,
            is_in_master_path: false,
            reclaimable_bytes: 24,
            evaluated_at: Utc::now(),
            replication: None,
        }),
        visual: None,
    };
    sink.save_batch(&[unmastered_doc]).await.unwrap();

    let mut groups = HashMap::new();
    groups.insert(
        "primary".to_string(),
        DriveGroupSpec {
            priority: 100,
            description: Some("Primary".to_string()),
        },
    );

    let config = DriveConfiguration {
        version: "1.0".to_string(),
        groups,
        drives: vec![DriveDefinition {
            id: "primary_main".to_string(),
            mount_path: primary_mount,
            master_path: primary_master.clone(),
            group: "primary".to_string(),
        }],
        rules: DriveRules::default(),
    };

    let service = MasterIngestService::new();
    // Empty source_drive option simulates running `cargo run -- ingest` without arguments
    let opts = IngestOptions {
        source_drive: String::new(),
        target_drive: None,
        dry_run: false,
        delete_source: false,
        verify_hash: true,
        buffer_config: Default::default(),
    };

    let report = service.execute_ingest(&sink, &config, opts).await.unwrap();
    assert_eq!(report.files_transferred, 1);
    assert_eq!(report.source_drive_id, "database");

    // Verify canonical master file was created in primary master folder
    let masters = sink.query_canonical_masters().await.unwrap();
    assert_eq!(masters.len(), 1);
    assert!(masters[0].file_context.file_path.exists());
    assert!(masters[0].file_context.file_path.starts_with(&primary_master));
}

#[tokio::test]
async fn test_master_ingest_pool_spillover_capacity_aware() {
    use metadata_extractor::infrastructure::disk_probe::MockDiskSpaceProbe;

    let temp_root = tempdir().unwrap();
    let vol1_mount = temp_root.path().join("primary_vol1");
    let vol1_master = vol1_mount.join("SPSA").join("media").join("master");
    let vol2_mount = temp_root.path().join("primary_vol2");
    let vol2_master = vol2_mount.join("SPSA").join("media").join("master");

    std::fs::create_dir_all(&vol1_master).unwrap();
    std::fs::create_dir_all(&vol2_master).unwrap();

    let staging_mount = temp_root.path().join("staging");
    std::fs::create_dir_all(&staging_mount).unwrap();
    let test_file = staging_mount.join("test_spillover.jpg");
    std::fs::write(&test_file, b"sample image data for spillover test").unwrap();

    let mut groups = HashMap::new();
    groups.insert(
        "primary".to_string(),
        DriveGroupSpec {
            priority: 100,
            description: Some("Primary Pool".to_string()),
        },
    );

    let config = DriveConfiguration {
        version: "1.0".to_string(),
        groups,
        drives: vec![
            DriveDefinition {
                id: "primary_vol1".to_string(),
                mount_path: vol1_mount.clone(),
                master_path: vol1_master.clone(),
                group: "primary".to_string(),
            },
            DriveDefinition {
                id: "primary_vol2".to_string(),
                mount_path: vol2_mount.clone(),
                master_path: vol2_master.clone(),
                group: "primary".to_string(),
            },
        ],
        rules: DriveRules::default(),
    };

    // Configure mock disk probe: vol1 is full (only 1 MB free, well below 5 GB buffer), vol2 has 100 GB
    let probe = Arc::new(MockDiskSpaceProbe::new());
    probe.set_available_space(&vol1_mount, 1024 * 1024);
    probe.set_available_space(&vol2_mount, 100 * 1024 * 1024 * 1024);

    let service = MasterIngestService::new()
        .with_probe(probe)
        .with_min_free_space_buffer(5 * 1024 * 1024 * 1024);

    let doc = MediaMetadataDocument {
        id: Uuid::new_v4(),
        file_context: FileContext {
            scanned_root: staging_mount.clone(),
            file_path: test_file.clone(),
            relative_path: PathBuf::from("test_spillover.jpg"),
            file_name: "test_spillover.jpg".to_string(),
            file_size_bytes: 36,
            extension: "jpg".to_string(),
            media_category: MediaCategory::Photo,
            modified_at: Some(Utc::now()),
        },
        temporal: TemporalMetadata::from_utc_datetime(
            Utc::now(),
            TimestampSource::ExifDateTimeOriginal,
        ),
        camera: None,
        exposure: None,
        location: None,
        dimensions: None,
        video: None,
        fingerprint: None,
        extraction_status: Default::default(),
        dedup: Some(DeduplicationMetadata {
            cluster_id: Uuid::new_v4(),
            status: DeduplicationStatus::NeedsIngestToMaster,
            canonical_master_id: None,
            canonical_master_path: None,
            drive_id: None,
            drive_group: None,
            is_in_master_path: false,
            reclaimable_bytes: 0,
            evaluated_at: Utc::now(),
            replication: None,
        }),
        visual: None,
    };

    let sink = MockIngestStorageSink::default();
    sink.save_batch(&[doc.clone()]).await.unwrap();

    let opts = IngestOptions {
        source_drive: staging_mount.to_string_lossy().to_string(),
        target_drive: Some("primary".to_string()),
        dry_run: false,
        delete_source: false,
        verify_hash: true,
        buffer_config: Default::default(),
    };

    let plan = service.compile_plan(&sink, &config, &opts).await.unwrap();
    assert_eq!(plan.tasks.len(), 1);
    // Task must have spilled over to primary_vol2 because primary_vol1 has < 5 GB free!
    assert!(
        plan.tasks[0].target_path.starts_with(&vol2_master),
        "Expected target path to start with vol2 master path {:?}, got {:?}",
        vol2_master,
        plan.tasks[0].target_path
    );

    let report = service.execute_ingest_plan(&sink, &config, plan, opts).await.unwrap();
    assert_eq!(report.files_transferred, 1);

    // Verify written file exists on vol2
    let masters = sink.query_canonical_masters().await.unwrap();
    assert_eq!(masters.len(), 1);
    assert_eq!(masters[0].dedup.as_ref().unwrap().drive_id.as_deref(), Some("primary_vol2"));
    assert!(masters[0].file_context.file_path.starts_with(&vol2_master));
    assert!(masters[0].file_context.file_path.exists());
}
