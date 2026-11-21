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
    DriveGroupSpec, DriveRules, MasterReplicationState, ReplicationStatus,
};
use metadata_extractor::domain::errors::StorageError;
use metadata_extractor::domain::metadata_models::{FilterEvaluationResult, MediaMetadataDocument};
use metadata_extractor::domain::models::DiscoveredMediaItem;
use metadata_extractor::domain::ports::{BatchWriteStats, MetadataStorageSink};

#[derive(Default, Clone)]
pub struct MockVobStorageSink {
    pub documents: Arc<Mutex<HashMap<Uuid, MediaMetadataDocument>>>,
}

#[async_trait::async_trait]
impl MetadataStorageSink for MockVobStorageSink {
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
        let mut modified = 0;
        for (id, meta) in updates {
            if let Some(doc) = guard.get_mut(id) {
                doc.dedup = Some(meta.clone());
                modified += 1;
            }
        }
        Ok(modified)
    }

    async fn query_unmastered_candidates(
        &self,
        source_mount: Option<&str>,
    ) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        let guard = self.documents.lock().unwrap();
        let candidates = guard
            .values()
            .filter(|d| {
                if let Some(ref d_meta) = d.dedup {
                    if d_meta.status != DeduplicationStatus::NeedsIngestToMaster {
                        return false;
                    }
                } else {
                    return false;
                }
                if let Some(mount) = source_mount {
                    d.file_context.scanned_root.to_string_lossy().starts_with(mount)
                        || d.file_context.file_path.to_string_lossy().starts_with(mount)
                } else {
                    true
                }
            })
            .cloned()
            .collect();
        Ok(candidates)
    }

    async fn query_removal_candidates(
        &self,
        mount_prefix: Option<&str>,
    ) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        let guard = self.documents.lock().unwrap();
        let candidates = guard
            .values()
            .filter(|d| {
                if let Some(ref d_meta) = d.dedup {
                    if d_meta.status != DeduplicationStatus::MarkForRemoval {
                        return false;
                    }
                } else {
                    return false;
                }
                if let Some(prefix) = mount_prefix {
                    d.file_context
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

    async fn find_document_by_id(&self, id: Uuid) -> Result<Option<MediaMetadataDocument>, StorageError> {
        let guard = self.documents.lock().unwrap();
        Ok(guard.get(&id).cloned())
    }

    async fn mark_removed(&self, id: Uuid) -> Result<(), StorageError> {
        let mut guard = self.documents.lock().unwrap();
        if let Some(doc) = guard.get_mut(&id) {
            if let Some(ref mut d) = doc.dedup {
                d.status = DeduplicationStatus::Removed;
            }
        }
        Ok(())
    }

    async fn delete_documents(&self, ids: &[Uuid]) -> Result<usize, StorageError> {
        let mut guard = self.documents.lock().unwrap();
        let mut count = 0;
        for id in ids {
            if guard.remove(id).is_some() {
                count += 1;
            }
        }
        Ok(count)
    }

    async fn delete_document(&self, id: Uuid) -> Result<bool, StorageError> {
        let mut guard = self.documents.lock().unwrap();
        Ok(guard.remove(&id).is_some())
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

    async fn purge_zero_byte_records(
        &self,
        _mount_prefix: Option<&str>,
    ) -> Result<usize, StorageError> {
        let mut guard = self.documents.lock().unwrap();
        let initial_len = guard.len();
        guard.retain(|_, d| d.file_context.file_size_bytes > 0);
        Ok(initial_len - guard.len())
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

    async fn link_canonical_masters(
        &self,
        _links: &[(String, Uuid, PathBuf)],
    ) -> Result<usize, StorageError> {
        Ok(0)
    }
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

fn find_file_recursively(dir: &Path, target_name: &str) -> Option<PathBuf> {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name == target_name {
                        return Some(path);
                    }
                }
            } else if path.is_dir() {
                if let Some(found) = find_file_recursively(&path, target_name) {
                    return Some(found);
                }
            }
        }
    }
    None
}

#[tokio::test]
async fn test_vob_and_dvd_metadata_ingestion_replication_and_pruning() {
    let tmp = tempdir().unwrap();
    let primary_dir = tmp.path().join("SP-1");
    let replica_dir = tmp.path().join("SP-2");
    let staging_dir = tmp.path().join("STAGING");
    let unorganized_dir = staging_dir.join("DVD_RIP").join("VIDEO_TS");

    fs::create_dir_all(primary_dir.join("media/master")).unwrap();
    fs::create_dir_all(replica_dir.join("media/master")).unwrap();
    fs::create_dir_all(&unorganized_dir).unwrap();

    // 1. Create VTS_01_1.VOB and its companion VTS_01_0.IFO and VTS_01_0.BUP files
    let vob_path = unorganized_dir.join("VTS_01_1.VOB");
    let ifo_path = unorganized_dir.join("VTS_01_0.IFO");
    let bup_path = unorganized_dir.join("VTS_01_0.BUP");

    fs::write(&vob_path, b"MPEG2_VIDEO_OBJECT_DATA_VTS_01_1").unwrap();
    fs::write(&ifo_path, b"DVD_INFO_NAVIGATION_DATA_VTS_01_0").unwrap();
    fs::write(&bup_path, b"DVD_BACKUP_NAVIGATION_DATA_VTS_01_0").unwrap();

    let config = create_test_config(&primary_dir, &replica_dir);
    let sink: Arc<dyn MetadataStorageSink> = Arc::new(MockVobStorageSink::default());
    let service = OrganizePipelineService::new();

    let options = OrganizePipelineOptions {
        source_path: Some(unorganized_dir.clone()),
        dry_run: false,
        skip_prune: false,
        no_verify_hash: false,
        buffer_size: "1M".to_string(),
        recheck_disk_hash: false,
        force_reindex: false,
        batch_size: 100,
        channel_capacity: 100,
        quiet: true,
        keep_source_dir: false,
        require_master_disk: false,
        recheck_all: false,
    };

    // 2. Execute end-to-end organize pipeline
    let report = service
        .execute(&sink, &config, options)
        .await
        .unwrap();

    assert_eq!(report.files_scanned, 3);
    assert_eq!(report.files_ingested, 3);
    assert_eq!(report.files_pruned, 3);

    // 3. Verify files on canonical master: all must be inside Videos/
    let master_dir = primary_dir.join("media/master");
    let master_vob = find_file_recursively(&master_dir, "VTS_01_1.VOB")
        .expect("Master must contain VTS_01_1.VOB");
    let master_ifo = find_file_recursively(&master_dir, "VTS_01_0.IFO")
        .expect("Master must contain VTS_01_0.IFO");
    let master_bup = find_file_recursively(&master_dir, "VTS_01_0.BUP")
        .expect("Master must contain VTS_01_0.BUP");

    assert_eq!(
        master_vob.parent().unwrap().file_name().unwrap(),
        "Videos",
        "VOB must be inside Videos/"
    );
    assert_eq!(
        master_ifo.parent().unwrap().file_name().unwrap(),
        "Videos",
        "IFO must be inside Videos/"
    );
    assert_eq!(
        master_bup.parent().unwrap().file_name().unwrap(),
        "Videos",
        "BUP must be inside Videos/"
    );
    assert_eq!(
        master_vob.parent().unwrap(),
        master_ifo.parent().unwrap(),
        "IFO must be co-located in same folder as VOB"
    );
    assert_eq!(
        master_vob.parent().unwrap(),
        master_bup.parent().unwrap(),
        "BUP must be co-located in same folder as VOB"
    );

    // 4. Verify replication to replica drive
    let replica_master_dir = replica_dir.join("media/master");
    let replica_vob = find_file_recursively(&replica_master_dir, "VTS_01_1.VOB")
        .expect("Replica must contain VTS_01_1.VOB");
    let replica_ifo = find_file_recursively(&replica_master_dir, "VTS_01_0.IFO")
        .expect("Replica must contain VTS_01_0.IFO");
    let replica_bup = find_file_recursively(&replica_master_dir, "VTS_01_0.BUP")
        .expect("Replica must contain VTS_01_0.BUP");

    assert_eq!(
        replica_vob.parent().unwrap().file_name().unwrap(),
        "Videos"
    );
    assert_eq!(
        replica_ifo.parent().unwrap().file_name().unwrap(),
        "Videos"
    );
    assert_eq!(
        replica_bup.parent().unwrap().file_name().unwrap(),
        "Videos"
    );

    // 5. Verify source files and empty source directories were pruned
    assert!(!vob_path.exists(), "Source VOB must be pruned");
    assert!(!ifo_path.exists(), "Source IFO must be pruned");
    assert!(!bup_path.exists(), "Source BUP must be pruned");
    assert!(!unorganized_dir.exists(), "Source directory VIDEO_TS must be pruned");
}

#[tokio::test]
async fn test_video_ts_root_ingestion_and_co_location() {
    let tmp = tempdir().unwrap();
    let primary_dir = tmp.path().join("SP-1");
    let replica_dir = tmp.path().join("SP-2");
    let staging_dir = tmp.path().join("STAGING");
    let unorganized_dir = staging_dir.join("DISC_ROOT").join("VIDEO_TS");

    fs::create_dir_all(primary_dir.join("media/master")).unwrap();
    fs::create_dir_all(replica_dir.join("media/master")).unwrap();
    fs::create_dir_all(&unorganized_dir).unwrap();

    // Create VIDEO_TS.VOB, VIDEO_TS.IFO, VIDEO_TS.BUP
    let vob_path = unorganized_dir.join("VIDEO_TS.VOB");
    let ifo_path = unorganized_dir.join("VIDEO_TS.IFO");
    let bup_path = unorganized_dir.join("VIDEO_TS.BUP");

    fs::write(&vob_path, b"MPEG2_VIDEO_TS_VOB").unwrap();
    fs::write(&ifo_path, b"DVD_INFO_VIDEO_TS_IFO").unwrap();
    fs::write(&bup_path, b"DVD_BACKUP_VIDEO_TS_BUP").unwrap();

    let config = create_test_config(&primary_dir, &replica_dir);
    let sink: Arc<dyn MetadataStorageSink> = Arc::new(MockVobStorageSink::default());
    let service = OrganizePipelineService::new();

    let options = OrganizePipelineOptions {
        source_path: Some(unorganized_dir.clone()),
        dry_run: false,
        skip_prune: false,
        no_verify_hash: false,
        buffer_size: "1M".to_string(),
        recheck_disk_hash: false,
        force_reindex: false,
        batch_size: 100,
        channel_capacity: 100,
        quiet: true,
        keep_source_dir: false,
        require_master_disk: false,
        recheck_all: false,
    };

    let report = service
        .execute(&sink, &config, options)
        .await
        .unwrap();

    assert_eq!(report.files_scanned, 3);
    assert_eq!(report.files_ingested, 3);
    assert_eq!(report.files_pruned, 3);

    let master_dir = primary_dir.join("media/master");
    let master_vob = find_file_recursively(&master_dir, "VIDEO_TS.VOB")
        .expect("Master must contain VIDEO_TS.VOB");
    let master_ifo = find_file_recursively(&master_dir, "VIDEO_TS.IFO")
        .expect("Master must contain VIDEO_TS.IFO");
    let master_bup = find_file_recursively(&master_dir, "VIDEO_TS.BUP")
        .expect("Master must contain VIDEO_TS.BUP");

    assert_eq!(master_vob.parent().unwrap().file_name().unwrap(), "Videos");
    assert_eq!(master_ifo.parent().unwrap().file_name().unwrap(), "Videos");
    assert_eq!(master_bup.parent().unwrap().file_name().unwrap(), "Videos");

    assert_eq!(master_vob.parent().unwrap(), master_ifo.parent().unwrap());
    assert_eq!(master_vob.parent().unwrap(), master_bup.parent().unwrap());

    assert!(!vob_path.exists());
    assert!(!ifo_path.exists());
    assert!(!bup_path.exists());
}
