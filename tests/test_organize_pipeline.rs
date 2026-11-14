use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use uuid::Uuid;

use metadata_extractor::application::organize_service::{
    OrganizePipelineOptions, OrganizePipelineService,
};
use metadata_extractor::domain::dedup_models::{
    DeduplicationMetadata, DeduplicationStatus, DriveConfiguration, DriveDefinition,
    DriveGroupSpec, DriveRules,
};
use metadata_extractor::domain::errors::{DeduplicationError, StorageError};
use metadata_extractor::domain::metadata_models::{FilterEvaluationResult, MediaMetadataDocument};
use metadata_extractor::domain::models::DiscoveredMediaItem;
use metadata_extractor::domain::ports::{BatchWriteStats, MetadataStorageSink};

#[derive(Default, Clone)]
pub struct MockOrganizeStorageSink {
    pub documents: Arc<Mutex<HashMap<Uuid, MediaMetadataDocument>>>,
}

#[async_trait::async_trait]
impl MetadataStorageSink for MockOrganizeStorageSink {
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
        items: &[DiscoveredMediaItem],
    ) -> Result<FilterEvaluationResult, StorageError> {
        let guard = self.documents.lock().unwrap();
        let mut unmodified = Vec::new();
        let mut to_extract = Vec::new();
        let mut skipped_bytes = 0;
        let mut extract_bytes = 0;

        for item in items {
            let found = guard.values().any(|d| {
                d.file_context.file_path == item.path
                    && d.file_context.file_size_bytes == item.file_size_bytes
            });
            if found {
                skipped_bytes += item.file_size_bytes;
                unmodified.push(item.clone());
            } else {
                extract_bytes += item.file_size_bytes;
                to_extract.push(item.clone());
            }
        }

        Ok(FilterEvaluationResult {
            unmodified_items: unmodified,
            items_to_extract: to_extract,
            skipped_bytes,
            extract_bytes,
        })
    }

    async fn query_duplicate_groups(
        &self,
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
        let dupes = groups
            .into_iter()
            .filter(|(_, docs)| docs.len() > 1)
            .collect();
        Ok(dupes)
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
        let mut guard = self.documents.lock().unwrap();
        if let Some(doc) = guard.get_mut(&doc_id) {
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
        mount_prefix: Option<&str>,
    ) -> Result<Vec<(String, Vec<MediaMetadataDocument>)>, StorageError> {
        let guard = self.documents.lock().unwrap();
        let mut groups: HashMap<String, Vec<MediaMetadataDocument>> = HashMap::new();
        for doc in guard.values() {
            if let Some(prefix) = mount_prefix {
                if !doc
                    .file_context
                    .file_path
                    .to_string_lossy()
                    .starts_with(prefix)
                {
                    continue;
                }
            }
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

    async fn update_master_replication_state(
        &self,
        master_id: Uuid,
        replication_state: &metadata_extractor::domain::dedup_models::MasterReplicationState,
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
        status: metadata_extractor::domain::dedup_models::ReplicationStatus,
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

    async fn link_canonical_masters(
        &self,
        _links: &[(String, Uuid, PathBuf)],
    ) -> Result<usize, StorageError> {
        Ok(0)
    }
}

fn count_files_in_dir(dir: &Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                count += 1;
            } else if path.is_dir() {
                count += count_files_in_dir(&path);
            }
        }
    }
    count
}

fn create_test_config(primary_dir: &Path, replica_dir: &Path) -> DriveConfiguration {
    let mut groups = HashMap::new();
    groups.insert(
        "primary".to_string(),
        DriveGroupSpec {
            priority: 100,
            description: Some("Primary Master Group".to_string()),
        },
    );
    groups.insert(
        "replica".to_string(),
        DriveGroupSpec {
            priority: 80,
            description: Some("Mirror Replica Group".to_string()),
        },
    );

    let drives = vec![
        DriveDefinition {
            id: "primary_main".to_string(),
            group: "primary".to_string(),
            mount_path: primary_dir.to_path_buf(),
            master_path: primary_dir.join("media/master"),
        },
        DriveDefinition {
            id: "replica_1".to_string(),
            group: "replica".to_string(),
            mount_path: replica_dir.to_path_buf(),
            master_path: replica_dir.join("media/master"),
        },
    ];

    DriveConfiguration {
        version: "1.0".to_string(),
        groups,
        drives,
        rules: DriveRules::default(),
    }
}

#[tokio::test]
async fn test_organize_pipeline_protected_source_rejected() {
    let temp = tempdir().unwrap();
    let primary_dir = temp.path().join("SP-1");
    let replica_dir = temp.path().join("SP-2");
    fs::create_dir_all(primary_dir.join("media/master")).unwrap();
    fs::create_dir_all(replica_dir.join("media/master")).unwrap();

    let config = create_test_config(&primary_dir, &replica_dir);
    let sink: Arc<dyn MetadataStorageSink> = Arc::new(MockOrganizeStorageSink::default());
    let service = OrganizePipelineService::new();

    // Passing primary master path as source
    let options = OrganizePipelineOptions {
        source_path: primary_dir.join("media/master"),
        dry_run: true,
        skip_prune: false,
        no_verify_hash: false,
        buffer_size: "1M".to_string(),
        recheck_disk_hash: false,
        force_reindex: false,
        batch_size: 100,
        channel_capacity: 100,
        quiet: true,
    };

    let result = service.execute(&sink, &config, options).await;
    assert!(result.is_err());
    match result.err().unwrap() {
        DeduplicationError::PruneForbiddenOnProtectedArchive { .. } => {}
        other => panic!("Expected PruneForbiddenOnProtectedArchive, got {:?}", other),
    }
}

#[tokio::test]
async fn test_organize_pipeline_missing_source_rejected() {
    let temp = tempdir().unwrap();
    let primary_dir = temp.path().join("SP-1");
    let replica_dir = temp.path().join("SP-2");
    fs::create_dir_all(primary_dir.join("media/master")).unwrap();
    fs::create_dir_all(replica_dir.join("media/master")).unwrap();

    let config = create_test_config(&primary_dir, &replica_dir);
    let sink: Arc<dyn MetadataStorageSink> = Arc::new(MockOrganizeStorageSink::default());
    let service = OrganizePipelineService::new();

    let options = OrganizePipelineOptions {
        source_path: temp.path().join("non_existent_folder"),
        dry_run: true,
        skip_prune: false,
        no_verify_hash: false,
        buffer_size: "1M".to_string(),
        recheck_disk_hash: false,
        force_reindex: false,
        batch_size: 100,
        channel_capacity: 100,
        quiet: true,
    };

    let result = service.execute(&sink, &config, options).await;
    assert!(result.is_err());
    match result.err().unwrap() {
        DeduplicationError::DriveNotMounted { .. } => {}
        other => panic!("Expected DriveNotMounted, got {:?}", other),
    }
}

#[tokio::test]
async fn test_organize_pipeline_dry_run_and_execution() {
    let temp = tempdir().unwrap();
    let primary_dir = temp.path().join("SP-1");
    let replica_dir = temp.path().join("SP-2");
    let unorganized_dir = primary_dir.join("unorganized");

    fs::create_dir_all(primary_dir.join("media/master")).unwrap();
    fs::create_dir_all(replica_dir.join("media/master")).unwrap();
    fs::create_dir_all(unorganized_dir.join("subfolder")).unwrap();

    // Create a dummy image file in unorganized/subfolder
    let test_file = unorganized_dir.join("subfolder/sample.jpg");
    // Minimal valid JPEG content so scanner extracts it
    let jpeg_data = [
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01, 0x01, 0x00, 0x00,
        0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06, 0x07, 0x06,
        0x05, 0x08, 0x07, 0x07, 0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D, 0x0C, 0x0B, 0x0B,
        0x0C, 0x19, 0x12, 0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D, 0x1A, 0x1C, 0x1C, 0x20,
        0x24, 0x2E, 0x27, 0x20, 0x22, 0x2C, 0x23, 0x1C, 0x1C, 0x28, 0x37, 0x29, 0x2C, 0x30, 0x31,
        0x34, 0x34, 0x34, 0x1F, 0x27, 0x39, 0x3D, 0x38, 0x32, 0x3C, 0x2E, 0x33, 0x34, 0x32, 0xFF,
        0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00,
        0x1F, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
        0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0x7F, 0x00, 0xFF, 0xD9,
    ];
    fs::write(&test_file, jpeg_data).unwrap();

    let config = create_test_config(&primary_dir, &replica_dir);
    let sink: Arc<dyn MetadataStorageSink> = Arc::new(MockOrganizeStorageSink::default());
    let service = OrganizePipelineService::new();

    // Step 1: Run Dry Run
    let dry_options = OrganizePipelineOptions {
        source_path: unorganized_dir.clone(),
        dry_run: true,
        skip_prune: false,
        no_verify_hash: false,
        buffer_size: "1M".to_string(),
        recheck_disk_hash: false,
        force_reindex: false,
        batch_size: 100,
        channel_capacity: 100,
        quiet: true,
    };

    let dry_report = service
        .execute(&sink, &config, dry_options)
        .await
        .expect("Dry run should succeed");

    assert!(dry_report.is_dry_run);
    assert_eq!(dry_report.files_scanned, 1);
    assert_eq!(dry_report.files_extracted, 1);
    assert_eq!(dry_report.files_pruned, 0);
    // In dry run, file must still exist
    assert!(test_file.exists());

    // Step 2: Run Live Execution
    let live_options = OrganizePipelineOptions {
        source_path: unorganized_dir.clone(),
        dry_run: false,
        skip_prune: false,
        no_verify_hash: false,
        buffer_size: "1M".to_string(),
        recheck_disk_hash: false,
        force_reindex: false,
        batch_size: 100,
        channel_capacity: 100,
        quiet: true,
    };

    let live_report = service
        .execute(&sink, &config, live_options)
        .await
        .expect("Live run should succeed");

    assert!(!live_report.is_dry_run);
    assert_eq!(live_report.files_ingested, 1);
    assert_eq!(live_report.files_replicated, 1);
    assert_eq!(live_report.files_pruned, 1);
    assert_eq!(live_report.empty_directories_removed, 1);

    // Source file should now be deleted
    assert!(!test_file.exists());
    // Empty subfolder should be deleted
    assert!(!unorganized_dir.join("subfolder").exists());
    // But unorganized_dir itself (source root) MUST be preserved!
    assert!(unorganized_dir.exists());

    // Verify Primary Master has the file
    let primary_files = count_files_in_dir(&primary_dir.join("media/master"));
    assert_eq!(primary_files, 1);

    // Verify Replica has the file
    let replica_files = count_files_in_dir(&replica_dir.join("media/master"));
    assert_eq!(replica_files, 1);
}

#[tokio::test]
async fn test_organize_pipeline_skip_prune() {
    let temp = tempdir().unwrap();
    let primary_dir = temp.path().join("SP-1");
    let replica_dir = temp.path().join("SP-2");
    let unorganized_dir = primary_dir.join("unorganized");

    fs::create_dir_all(primary_dir.join("media/master")).unwrap();
    fs::create_dir_all(replica_dir.join("media/master")).unwrap();
    fs::create_dir_all(&unorganized_dir).unwrap();

    let test_file = unorganized_dir.join("sample.jpg");
    let jpeg_data = [
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01, 0x01, 0x00, 0x00,
        0x01, 0x00, 0x01, 0x00, 0x00, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06, 0x07, 0x06,
        0x05, 0x08, 0x07, 0x07, 0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D, 0x0C, 0x0B, 0x0B,
        0x0C, 0x19, 0x12, 0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D, 0x1A, 0x1C, 0x1C, 0x20,
        0x24, 0x2E, 0x27, 0x20, 0x22, 0x2C, 0x23, 0x1C, 0x1C, 0x28, 0x37, 0x29, 0x2C, 0x30, 0x31,
        0x34, 0x34, 0x34, 0x1F, 0x27, 0x39, 0x3D, 0x38, 0x32, 0x3C, 0x2E, 0x33, 0x34, 0x32, 0xFF,
        0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00, 0xFF, 0xC4, 0x00,
        0x1F, 0x00, 0x00, 0x01, 0x05, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B,
        0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00, 0x7F, 0x00, 0xFF, 0xD9,
    ];
    fs::write(&test_file, jpeg_data).unwrap();

    let config = create_test_config(&primary_dir, &replica_dir);
    let sink: Arc<dyn MetadataStorageSink> = Arc::new(MockOrganizeStorageSink::default());
    let service = OrganizePipelineService::new();

    let options = OrganizePipelineOptions {
        source_path: unorganized_dir.clone(),
        dry_run: false,
        skip_prune: true,
        no_verify_hash: false,
        buffer_size: "1M".to_string(),
        recheck_disk_hash: false,
        force_reindex: false,
        batch_size: 100,
        channel_capacity: 100,
        quiet: true,
    };

    let report = service
        .execute(&sink, &config, options)
        .await
        .expect("Execution should succeed");

    assert_eq!(report.files_ingested, 1);
    assert_eq!(report.files_replicated, 1);
    assert_eq!(report.files_pruned, 0);
    // Source file must be kept because skip_prune was set
    assert!(test_file.exists());
}
