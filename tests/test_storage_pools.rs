use chrono::Utc;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use uuid::Uuid;

use metadata_extractor::application::dedup_engine::MediaDeduplicationEngine;
use metadata_extractor::application::drive_pruner::{DrivePrunerOptions, DrivePruningService};
use metadata_extractor::application::ingest_service::MasterIngestService;
use metadata_extractor::application::replica_service::{ReplicaSyncOptions, ReplicaSyncService};
use metadata_extractor::domain::dedup_models::{
    DeduplicationMetadata, DeduplicationStatus, DriveConfiguration, DriveDefinition,
    DriveGroupSpec, DriveRules, IngestOptions, MasterReplicationState,
};
use metadata_extractor::domain::errors::StorageError;
use metadata_extractor::domain::metadata_models::{
    FileContext, FilterEvaluationResult, MediaMetadataDocument, TemporalMetadata,
    TimestampSource,
};
use metadata_extractor::domain::models::{DiscoveredMediaItem, MediaCategory};
use metadata_extractor::domain::ports::{BatchWriteStats, MetadataStorageSink};
use metadata_extractor::infrastructure::archive_guard::ArchiveProtectionGuard;
use metadata_extractor::infrastructure::disk_probe::MockDiskSpaceProbe;
use metadata_extractor::infrastructure::drives_config::CanonicalPathMatcher;

#[derive(Default, Clone)]
struct EndToEndMockSink {
    documents: Arc<Mutex<HashMap<Uuid, MediaMetadataDocument>>>,
    marked_removed: Arc<Mutex<Vec<Uuid>>>,
}

#[async_trait::async_trait]
impl MetadataStorageSink for EndToEndMockSink {
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
}

#[tokio::test]
async fn test_storage_pools_complete_end_to_end_lifecycle() {
    let temp_root = tempdir().unwrap();

    // 1. Setup primary pool: primary_vol1 and primary_vol2
    let pri1_mount = temp_root.path().join("primary_vol1");
    let pri1_master = pri1_mount.join("SPSA").join("media").join("master");
    let pri2_mount = temp_root.path().join("primary_vol2");
    let pri2_master = pri2_mount.join("SPSA").join("media").join("master");

    std::fs::create_dir_all(&pri1_master).unwrap();
    std::fs::create_dir_all(&pri2_master).unwrap();

    // 2. Setup replica pool: rep1_vol1 and rep1_vol2
    let rep1_mount = temp_root.path().join("rep1_vol1");
    let rep1_master = rep1_mount.join("SPSA").join("media").join("master");
    let rep2_mount = temp_root.path().join("rep1_vol2");
    let rep2_master = rep2_mount.join("SPSA").join("media").join("master");

    std::fs::create_dir_all(&rep1_master).unwrap();
    std::fs::create_dir_all(&rep2_master).unwrap();

    // 3. Setup unmastered staging directory
    let staging_mount = temp_root.path().join("staging");
    std::fs::create_dir_all(&staging_mount).unwrap();
    let staging_file = staging_mount.join("incoming_photo.jpg");
    std::fs::write(&staging_file, b"unique payload for e2e pool testing").unwrap();

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
                id: "primary_vol1".to_string(),
                mount_path: pri1_mount.clone(),
                master_path: pri1_master.clone(),
                group: "primary".to_string(),
            },
            DriveDefinition {
                id: "primary_vol2".to_string(),
                mount_path: pri2_mount.clone(),
                master_path: pri2_master.clone(),
                group: "primary".to_string(),
            },
            DriveDefinition {
                id: "rep1_vol1".to_string(),
                mount_path: rep1_mount.clone(),
                master_path: rep1_master.clone(),
                group: "replica1".to_string(),
            },
            DriveDefinition {
                id: "rep1_vol2".to_string(),
                mount_path: rep2_mount.clone(),
                master_path: rep2_master.clone(),
                group: "replica1".to_string(),
            },
        ],
        rules: DriveRules::default(),
    };

    // 4. Configure simulated disk space probe
    // primary_vol1 has only 1 MB (spills to primary_vol2 with 100 GB)
    // rep1_vol1 has only 1 MB (spills to rep1_vol2 with 100 GB)
    let probe = Arc::new(MockDiskSpaceProbe::new());
    probe.set_available_space(&pri1_mount, 1024 * 1024);
    probe.set_available_space(&pri2_mount, 100 * 1024 * 1024 * 1024);
    probe.set_available_space(&rep1_mount, 1024 * 1024);
    probe.set_available_space(&rep2_mount, 100 * 1024 * 1024 * 1024);

    let sink = EndToEndMockSink::default();

    // 5. Ingest unmastered file -> should spill over to primary_vol2
    let incoming_doc = MediaMetadataDocument {
        id: Uuid::new_v4(),
        file_context: FileContext {
            scanned_root: staging_mount.clone(),
            file_path: staging_file.clone(),
            relative_path: PathBuf::from("incoming_photo.jpg"),
            file_name: "incoming_photo.jpg".to_string(),
            file_size_bytes: 35,
            extension: "jpg".to_string(),
            media_category: MediaCategory::Photo,
            modified_at: Some(Utc::now()),
        },
        temporal: TemporalMetadata::from_utc_datetime(
            chrono::DateTime::from_timestamp(1767225600, 0).unwrap(), // 2026-01-01
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
    };

    sink.save_batch(&[incoming_doc]).await.unwrap();

    let ingest_service = MasterIngestService::new()
        .with_probe(probe.clone())
        .with_min_free_space_buffer(5 * 1024 * 1024 * 1024);

    let ingest_opts = IngestOptions {
        source_drive: staging_mount.to_string_lossy().to_string(),
        target_drive: Some("primary".to_string()),
        dry_run: false,
        delete_source: false,
        verify_hash: true,
        buffer_config: Default::default(),
    };

    let ingest_report = ingest_service
        .execute_ingest(&sink, &config, ingest_opts)
        .await
        .unwrap();
    assert_eq!(ingest_report.files_transferred, 1);

    // Verify written file is cataloged under primary_vol2 and exists on disk
    let masters = sink.query_canonical_masters().await.unwrap();
    assert_eq!(masters.len(), 1);
    let master = &masters[0];
    assert_eq!(master.dedup.as_ref().unwrap().drive_id.as_deref(), Some("primary_vol2"));
    assert!(master.file_context.file_path.starts_with(&pri2_master));
    assert!(master.file_context.file_path.exists());

    // 6. Test Deduplication Single-Copy Invariant across primary pool
    // Create an identical file on primary_vol1
    let pri1_file = pri1_master.join("2026/01-Jan-2026/Photos/incoming_photo.jpg");
    std::fs::create_dir_all(pri1_file.parent().unwrap()).unwrap();
    std::fs::write(&pri1_file, b"unique payload for e2e pool testing").unwrap();

    let matcher = CanonicalPathMatcher::new(config.clone());
    let pri1_doc = MediaMetadataDocument {
        id: Uuid::new_v4(),
        file_context: FileContext {
            scanned_root: pri1_master.clone(),
            file_path: pri1_file.clone(),
            relative_path: PathBuf::from("2026/01-Jan-2026/Photos/incoming_photo.jpg"),
            file_name: "incoming_photo.jpg".to_string(),
            file_size_bytes: 35,
            extension: "jpg".to_string(),
            media_category: MediaCategory::Photo,
            modified_at: Some(Utc::now()),
        },
        temporal: master.temporal.clone(),
        camera: None,
        exposure: None,
        location: None,
        dimensions: None,
        video: None,
        fingerprint: None,
        extraction_status: Default::default(),
        dedup: None,
    };

    let cluster = vec![pri1_doc.clone(), master.clone()];
    let evals = MediaDeduplicationEngine::evaluate_replica_cluster(&cluster, &matcher);
    let eval_map: HashMap<Uuid, DeduplicationMetadata> = evals.into_iter().collect();

    // Earliest declared drive (primary_vol1) is CanonicalMaster
    assert_eq!(
        eval_map.get(&pri1_doc.id).unwrap().status,
        DeduplicationStatus::CanonicalMaster
    );
    // Later declared drive (primary_vol2) is MarkForRemoval
    assert_eq!(
        eval_map.get(&master.id).unwrap().status,
        DeduplicationStatus::MarkForRemoval
    );

    // 7. Test Replica Synchronization with Spillover across replica pool
    let replica_service = ReplicaSyncService::new()
        .with_probe(probe)
        .with_min_free_space_buffer(5 * 1024 * 1024 * 1024);

    let replica_opts = ReplicaSyncOptions {
        target_replica_id: None,
        primary_drive_id: None,
        dry_run: false,
        verify_hash: true,
        buffer_config: Default::default(),
    };

    let replica_report = replica_service
        .execute_sync(&sink, &config, replica_opts)
        .await
        .unwrap();
    assert_eq!(replica_report.files_replicated, 1);

    // Verify replica was placed on rep1_vol2 (because rep1_vol1 had < 5 GB buffer)
    let rep2_file = rep2_master.join("2026/01-Jan-2026/Photos/incoming_photo.jpg");
    assert!(rep2_file.exists(), "Replicated file must exist on rep1_vol2");

    // 8. Test Comprehensive Archive Protection Across All Pool Volumes
    let guard = ArchiveProtectionGuard::from_config(&config);
    assert!(guard.is_protected_path(&pri1_file));
    assert!(guard.is_protected_path(&master.file_context.file_path));
    assert!(guard.is_protected_path(&rep2_file));

    let pruner = DrivePruningService::new();
    let prune_res = pruner
        .prune_drive(
            &sink,
            &config,
            DrivePrunerOptions::for_drive(pri2_master.to_string_lossy(), false),
        )
        .await;
    assert!(prune_res.is_err());
}
