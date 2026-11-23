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
use metadata_extractor::domain::errors::StorageError;
use metadata_extractor::domain::metadata_models::{FilterEvaluationResult, MediaMetadataDocument};
use metadata_extractor::domain::models::DiscoveredMediaItem;
use metadata_extractor::domain::ports::{BatchWriteStats, MetadataStorageSink};
use metadata_extractor::MediaFingerprinter;

#[derive(Default, Clone)]
pub struct MockSidecarStorageSink {
    pub documents: Arc<Mutex<HashMap<Uuid, MediaMetadataDocument>>>,
}

#[async_trait::async_trait]
impl MetadataStorageSink for MockSidecarStorageSink {
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

fn find_file_recursive(dir: &Path, file_name: &str) -> Option<PathBuf> {
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if name.eq_ignore_ascii_case(file_name) {
                        return Some(path);
                    }
                }
            } else if path.is_dir() {
                if let Some(found) = find_file_recursive(&path, file_name) {
                    return Some(found);
                }
            }
        }
    }
    None
}

#[tokio::test]
async fn test_aae_sidecar_ingestion_replication_and_pruning() {
    let temp = tempdir().unwrap();
    let primary_dir = temp.path().join("SP-1");
    let replica_dir = temp.path().join("SP-2");
    let unorganized_dir = primary_dir.join("unorganized");

    fs::create_dir_all(primary_dir.join("media/master")).unwrap();
    fs::create_dir_all(replica_dir.join("media/master")).unwrap();
    fs::create_dir_all(unorganized_dir.join("2024_trip")).unwrap();

    // 1. Create a media file and its companion .AAE sidecar in source folder
    let img_path = unorganized_dir.join("2024_trip/IMG_5001.JPG");
    let aae_path = unorganized_dir.join("2024_trip/IMG_5001.AAE");

    // Minimal valid JPEG content
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
    fs::write(&img_path, jpeg_data).unwrap();
    fs::write(&aae_path, b"<?xml version=\"1.0\"?><plist><dict><key>adjustmentData</key><string>filter_crop</string></dict></plist>").unwrap();

    let config = create_test_config(&primary_dir, &replica_dir);
    let sink: Arc<dyn MetadataStorageSink> = Arc::new(MockSidecarStorageSink::default());
    let service = OrganizePipelineService::new();

    // Run the organize pipeline
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
        keep_source_dir: true,
        require_master_disk: false,
        recheck_all: false,
    };

    let report = service
        .execute(&sink, &config, options)
        .await
        .expect("Organize pipeline should succeed");

    assert_eq!(report.files_scanned, 2);
    assert_eq!(report.files_ingested, 2);
    assert_eq!(report.files_replicated, 2);
    assert_eq!(report.files_pruned, 2);

    // Verify US1: Both IMG_5001.JPG and IMG_5001.AAE are co-located in the primary master directory
    let master_img = find_file_recursive(&primary_dir.join("media/master"), "IMG_5001.JPG");
    assert!(
        master_img.is_some(),
        "IMG_5001.JPG must exist in primary master"
    );
    let master_img_path = master_img.unwrap();

    let master_aae = master_img_path.parent().unwrap().join("IMG_5001.AAE");
    assert!(
        master_aae.is_file(),
        "IMG_5001.AAE must exist directly next to IMG_5001.JPG in primary master"
    );

    // Verify US2: Both IMG_5001.JPG and IMG_5001.AAE are replicated to replica drive
    let replica_img = find_file_recursive(&replica_dir.join("media/master"), "IMG_5001.JPG");
    assert!(
        replica_img.is_some(),
        "IMG_5001.JPG must exist on replica drive"
    );
    let replica_img_path = replica_img.unwrap();

    let replica_aae = replica_img_path.parent().unwrap().join("IMG_5001.AAE");
    assert!(
        replica_aae.is_file(),
        "IMG_5001.AAE must be replicated directly next to IMG_5001.JPG on replica drive"
    );

    // Verify US3: Both files were pruned from source, and the empty subdirectory was cleaned
    assert!(!img_path.exists(), "Source image must be pruned");
    assert!(!aae_path.exists(), "Source .AAE sidecar must be pruned");
    assert!(
        !unorganized_dir.join("2024_trip").exists(),
        "Empty subfolder must be removed"
    );
    assert!(
        unorganized_dir.exists(),
        "Source root directory must be preserved"
    );
}

#[tokio::test]
async fn test_video_companion_aae_co_located_in_videos_folder() {
    let temp = tempdir().unwrap();
    let primary_dir = temp.path().join("SP-1");
    let replica_dir = temp.path().join("SP-2");
    let unorganized_dir = primary_dir.join("unorganized");

    fs::create_dir_all(primary_dir.join("media/master")).unwrap();
    fs::create_dir_all(replica_dir.join("media/master")).unwrap();
    fs::create_dir_all(&unorganized_dir).unwrap();

    let vid_path = unorganized_dir.join("IMG_9001.MOV");
    let aae_path = unorganized_dir.join("IMG_9001.AAE");

    // Minimal valid QuickTime/MP4 container header so scanner recognizes it
    let mut mov_data = vec![0u8; 64];
    mov_data[0..4].copy_from_slice(&32u32.to_be_bytes());
    mov_data[4..8].copy_from_slice(b"ftyp");
    mov_data[8..12].copy_from_slice(b"qt  ");
    fs::write(&vid_path, mov_data).unwrap();
    fs::write(&aae_path, b"<plist><dict></dict></plist>").unwrap();

    let config = create_test_config(&primary_dir, &replica_dir);
    let sink: Arc<dyn MetadataStorageSink> = Arc::new(MockSidecarStorageSink::default());
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
        .expect("Organize pipeline should succeed");

    assert_eq!(report.files_ingested, 2);
    assert_eq!(report.files_replicated, 2);

    let master_vid =
        find_file_recursive(&primary_dir.join("media/master"), "IMG_9001.MOV").unwrap();
    assert!(
        master_vid.to_string_lossy().contains("Videos"),
        "Video must be placed in Videos folder"
    );

    let master_aae = master_vid.parent().unwrap().join("IMG_9001.AAE");
    assert!(
        master_aae.is_file(),
        "Companion AAE must be placed right next to the video in the Videos folder"
    );
}

#[tokio::test]
async fn test_delayed_aae_routed_to_existing_master_companion() {
    let temp = tempdir().unwrap();
    let primary_dir = temp.path().join("SP-1");
    let replica_dir = temp.path().join("SP-2");
    let unorganized_dir = primary_dir.join("unorganized");

    let master_photos_dir = primary_dir.join("media/master/2023/10-Oct-2023/Photos");
    fs::create_dir_all(&master_photos_dir).unwrap();
    fs::create_dir_all(replica_dir.join("media/master")).unwrap();
    fs::create_dir_all(&unorganized_dir).unwrap();

    // Existing master photo already present
    let existing_master_photo = master_photos_dir.join("IMG_7777.JPG");
    fs::write(&existing_master_photo, b"master_photo_content").unwrap();

    // Setup mock document for the existing master
    let sink: Arc<dyn MetadataStorageSink> = Arc::new(MockSidecarStorageSink::default());
    let master_doc = MediaMetadataDocument {
        id: Uuid::new_v4(),
        file_context: metadata_extractor::domain::metadata_models::FileContext {
            scanned_root: primary_dir.join("media/master"),
            file_path: existing_master_photo.clone(),
            relative_path: PathBuf::from("2023/10-Oct-2023/Photos/IMG_7777.JPG"),
            file_name: "IMG_7777.JPG".to_string(),
            file_size_bytes: 20,
            extension: "jpg".to_string(),
            media_category: metadata_extractor::domain::models::MediaCategory::Photo,
            modified_at: None,
        },
        temporal: metadata_extractor::domain::metadata_models::TemporalMetadata {
            timestamp_utc: None,
            iso_timestamp: "2023-10-10T00:00:00Z".to_string(),
            year: 2023,
            month: 10,
            day: 10,
            hour: 0,
            minute: 0,
            second: 0,
            source:
                metadata_extractor::domain::metadata_models::TimestampSource::ExifDateTimeOriginal,
        },
        camera: None,
        exposure: None,
        location: None,
        dimensions: None,
        video: None,
        fingerprint: Some(metadata_extractor::domain::dedup_models::MediaFingerprint {
            id: Uuid::new_v4(),
            file_path: existing_master_photo.clone(),
            file_size_bytes: 20,
            media_category: metadata_extractor::domain::models::MediaCategory::Photo,
            dimensions: None,
            sample_digest: None,
            cryptographic_hash: "dummy_master_hash".to_string(),
            perceptual_hash: None,
            phash_hex: None,
            rotated_hashes: None,
            is_low_entropy: false,
            fingerprinted_at: chrono::Utc::now(),
        }),
        extraction_status: metadata_extractor::domain::metadata_models::ExtractionStatus::default(),
        dedup: Some(DeduplicationMetadata {
            cluster_id: Uuid::new_v4(),
            status: DeduplicationStatus::CanonicalMaster,
            canonical_master_id: None,
            canonical_master_path: Some(existing_master_photo.clone()),
            drive_id: Some("primary_main".to_string()),
            drive_group: Some("primary".to_string()),
            is_in_master_path: true,
            reclaimable_bytes: 0,
            evaluated_at: chrono::Utc::now(),
            replication: None,
        }),
        visual: None,
    };
    sink.save_batch(&[master_doc]).await.unwrap();

    // Now unorganized folder ONLY contains the delayed AAE sidecar
    let delayed_aae = unorganized_dir.join("IMG_7777.AAE");
    fs::write(&delayed_aae, b"<plist><dict>delayed_edit</dict></plist>").unwrap();

    let config = create_test_config(&primary_dir, &replica_dir);
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
        .expect("Organize pipeline should succeed");

    assert_eq!(report.files_ingested, 1);

    // Verify the delayed AAE was routed directly into the existing master folder
    let expected_master_aae = master_photos_dir.join("IMG_7777.AAE");
    assert!(
        expected_master_aae.is_file(),
        "Delayed AAE must be placed in the existing master folder beside IMG_7777.JPG"
    );
}

#[tokio::test]
async fn test_skipped_identical_media_still_transfers_companion_aae() {
    let temp = tempdir().unwrap();
    let primary_dir = temp.path().join("SP-1");
    let replica_dir = temp.path().join("SP-2");
    let unorganized_dir = primary_dir.join("unorganized");

    let master_photos_dir = primary_dir.join("media/master/2026/02-May-2026/Photos");
    fs::create_dir_all(&master_photos_dir).unwrap();
    fs::create_dir_all(replica_dir.join("media/master")).unwrap();
    fs::create_dir_all(&unorganized_dir).unwrap();

    let existing_master_photo = master_photos_dir.join("IMG_8984.JPG");
    fs::write(&existing_master_photo, b"jpeg_master_content").unwrap();

    // Source contains identical JPEG and companion IMG_O8984.AAE sidecar
    let src_photo = unorganized_dir.join("IMG_8984.JPG");
    let src_aae = unorganized_dir.join("IMG_O8984.AAE");
    fs::write(&src_photo, b"jpeg_master_content").unwrap();
    fs::write(&src_aae, b"<plist><dict>edit_data</dict></plist>").unwrap();

    let config = create_test_config(&primary_dir, &replica_dir);
    let sink: Arc<dyn MetadataStorageSink> = Arc::new(MockSidecarStorageSink::default());

    // Register source photo as NeedsIngestToMaster
    let photo_doc = MediaMetadataDocument {
        id: Uuid::new_v4(),
        file_context: metadata_extractor::domain::metadata_models::FileContext {
            scanned_root: unorganized_dir.clone(),
            file_path: src_photo.clone(),
            relative_path: PathBuf::from("IMG_8984.JPG"),
            file_name: "IMG_8984.JPG".to_string(),
            file_size_bytes: 19,
            extension: "jpg".to_string(),
            media_category: metadata_extractor::domain::models::MediaCategory::Photo,
            modified_at: None,
        },
        temporal: metadata_extractor::domain::metadata_models::TemporalMetadata {
            timestamp_utc: None,
            iso_timestamp: "2026-05-02T10:00:00Z".to_string(),
            year: 2026,
            month: 5,
            day: 2,
            hour: 10,
            minute: 0,
            second: 0,
            source: metadata_extractor::domain::metadata_models::TimestampSource::ExifDateTimeOriginal,
        },
        camera: None,
        exposure: None,
        location: None,
        dimensions: None,
        video: None,
        fingerprint: Some(metadata_extractor::domain::dedup_models::MediaFingerprint {
            id: Uuid::new_v4(),
            file_path: src_photo.clone(),
            file_size_bytes: 19,
            media_category: metadata_extractor::domain::models::MediaCategory::Photo,
            dimensions: None,
            sample_digest: None,
            cryptographic_hash: metadata_extractor::infrastructure::blake3_hasher::Blake3MediaHasher::new()
                .compute_cryptographic_hash(&src_photo)
                .unwrap(),
            perceptual_hash: None,
            phash_hex: None,
            rotated_hashes: None,
            is_low_entropy: false,
            fingerprinted_at: chrono::Utc::now(),
        }),
        extraction_status: metadata_extractor::domain::metadata_models::ExtractionStatus::default(),
        dedup: Some(DeduplicationMetadata {
            cluster_id: Uuid::new_v4(),
            status: DeduplicationStatus::NeedsIngestToMaster,
            canonical_master_id: None,
            canonical_master_path: None,
            drive_id: Some("primary_main".to_string()),
            drive_group: Some("primary".to_string()),
            is_in_master_path: false,
            reclaimable_bytes: 0,
            evaluated_at: chrono::Utc::now(),
            replication: None,
        }),
        visual: None,
    };
    sink.save_batch(&[photo_doc]).await.unwrap();

    let ingest_service = metadata_extractor::application::ingest_service::MasterIngestService::new();
    let options = metadata_extractor::domain::dedup_models::IngestOptions {
        source_drive: unorganized_dir.to_string_lossy().to_string(),
        target_drive: None,
        dry_run: false,
        delete_source: false,
        verify_hash: false,
        buffer_config: metadata_extractor::domain::dedup_models::TransferBufferConfig::default(),
    };

    let plan = ingest_service
        .compile_plan(sink.as_ref(), &config, &options)
        .await
        .expect("Compile plan should succeed");

    assert_eq!(plan.tasks.len(), 1);
    assert_eq!(plan.tasks[0].status, metadata_extractor::domain::dedup_models::IngestTaskStatus::SkippedIdentical);

    let report = ingest_service
        .execute_ingest_plan(sink.as_ref(), &config, plan, options)
        .await
        .expect("Execution should succeed");

    assert_eq!(report.files_skipped_identical, 1);

    // Verify companion .AAE was transferred next to the master photo even though photo was SkippedIdentical!
    let sidecar_dest = master_photos_dir.join("IMG_8984.AAE");
    assert!(
        sidecar_dest.is_file(),
        "Companion AAE sidecar must be copied next to existing master photo"
    );
}

#[test]
fn test_resolve_drives_single_connected_drive_primary_or_secondary() {
    let temp = tempdir().unwrap();
    // Only SP-2 is mounted; SP-1 is NOT created on disk
    let sp1_dir = temp.path().join("SP-1_OFFLINE");
    let sp2_dir = temp.path().join("SP-2_MOUNTED");
    fs::create_dir_all(&sp2_dir).unwrap();

    let mut groups = std::collections::HashMap::new();
    groups.insert(
        "primary".to_string(),
        metadata_extractor::domain::dedup_models::DriveGroupSpec {
            priority: 100,
            description: Some("Primary Master".to_string()),
        },
    );
    groups.insert(
        "secondary".to_string(),
        metadata_extractor::domain::dedup_models::DriveGroupSpec {
            priority: 80,
            description: Some("Secondary Master".to_string()),
        },
    );

    let drives = vec![
        DriveDefinition {
            id: "SP-1".to_string(),
            group: "primary".to_string(),
            mount_path: sp1_dir.clone(),
            master_path: sp1_dir.join("media/master"),
        },
        DriveDefinition {
            id: "SP-2".to_string(),
            group: "secondary".to_string(),
            mount_path: sp2_dir.clone(),
            master_path: sp2_dir.join("media/master"),
        },
    ];

    let config = DriveConfiguration {
        version: "1.0".to_string(),
        groups,
        drives,
        rules: metadata_extractor::domain::dedup_models::DriveRules::default(),
    };

    // When source is SP-2 unorganized, and target is None (default):
    let unorganized = sp2_dir.join("unorganized");
    fs::create_dir_all(&unorganized).unwrap();

    let (source, target) = metadata_extractor::application::ingest_service::MasterIngestService::resolve_drives(
        &config,
        &unorganized.to_string_lossy(),
        None,
    )
    .expect("Should resolve drives when only SP-2 is connected");

    assert_eq!(source.mount_path, unorganized);
    // Since SP-1 is offline and SP-2 is mounted, target resolves to SP-2 without error!
    assert_eq!(target.id, "SP-2");
    assert_eq!(target.group, "secondary");

    // Also verify replica service resolves SP-2 when SP-1 is offline
    let (primary_res, _replicas) = metadata_extractor::application::replica_service::ReplicaSyncService::resolve_drives(
        &config,
        None,
        None,
    )
    .expect("ReplicaSyncService should resolve to mounted SP-2");
    assert_eq!(primary_res.id, "SP-2");
}

#[tokio::test]
async fn test_prune_drive_verifies_and_purges_redundant_aae_sidecar() {
    use metadata_extractor::application::drive_pruner::{DrivePruningService, DrivePrunerOptions};
    use metadata_extractor::domain::models::MediaCategory;

    let temp = tempdir().unwrap();
    let primary_dir = temp.path().join("primary");
    let unorganized_dir = temp.path().join("unorganized");
    let master_photos_dir = primary_dir.join("media/master/2026/30-Jan-2026/Photos");
    fs::create_dir_all(&master_photos_dir).unwrap();
    fs::create_dir_all(&unorganized_dir).unwrap();

    // 1. Create canonical master photo and companion .aae on disk
    let master_photo_path = master_photos_dir.join("IMG_8475.JPG");
    fs::write(&master_photo_path, b"MASTER_PHOTO_DATA").unwrap();
    let master_sidecar_path = master_photos_dir.join("IMG_8475.aae");
    fs::write(&master_sidecar_path, b"MASTER_SIDECAR_XML_V1").unwrap();

    let mut groups = std::collections::HashMap::new();
    groups.insert(
        "primary".to_string(),
        metadata_extractor::domain::dedup_models::DriveGroupSpec {
            priority: 100,
            description: Some("Primary".to_string()),
        },
    );
    groups.insert(
        "unorganized".to_string(),
        metadata_extractor::domain::dedup_models::DriveGroupSpec {
            priority: 10,
            description: Some("Unorganized".to_string()),
        },
    );

    let drives = vec![
        DriveDefinition {
            id: "primary".to_string(),
            group: "primary".to_string(),
            mount_path: primary_dir.clone(),
            master_path: primary_dir.join("media/master"),
        },
        DriveDefinition {
            id: "unorganized".to_string(),
            group: "unorganized".to_string(),
            mount_path: unorganized_dir.clone(),
            master_path: unorganized_dir.join("media/master"),
        },
    ];

    let config = DriveConfiguration {
        version: "1.0".to_string(),
        groups,
        drives,
        rules: metadata_extractor::domain::dedup_models::DriveRules::default(),
    };

    let sink = MockSidecarStorageSink::default();

    // 2. Insert canonical master document for master_sidecar
    let master_doc_id = Uuid::new_v4();
    let master_sidecar_doc = MediaMetadataDocument {
        id: master_doc_id,
        file_context: metadata_extractor::domain::metadata_models::FileContext {
            scanned_root: primary_dir.join("media/master"),
            file_path: master_sidecar_path.clone(),
            relative_path: PathBuf::from("2026/30-Jan-2026/Photos/IMG_8475.aae"),
            file_name: "IMG_8475.aae".to_string(),
            file_size_bytes: 21,
            extension: "aae".to_string(),
            media_category: MediaCategory::Sidecar,
            modified_at: None,
        },
        temporal: metadata_extractor::domain::metadata_models::TemporalMetadata::from_utc_datetime(
            chrono::Utc::now(),
            metadata_extractor::domain::metadata_models::TimestampSource::FileSystemModified,
        ),
        camera: None,
        exposure: None,
        location: None,
        dimensions: None,
        video: None,
        fingerprint: Some(metadata_extractor::domain::dedup_models::MediaFingerprint {
            id: Uuid::new_v4(),
            file_path: master_sidecar_path.clone(),
            file_size_bytes: 21,
            media_category: MediaCategory::Sidecar,
            dimensions: None,
            sample_digest: None,
            cryptographic_hash: "hash_master_sidecar".to_string(),
            perceptual_hash: None,
            phash_hex: None,
            rotated_hashes: None,
            is_low_entropy: false,
            fingerprinted_at: chrono::Utc::now(),
        }),
        extraction_status: Default::default(),
        dedup: Some(DeduplicationMetadata {
            cluster_id: Uuid::new_v4(),
            status: DeduplicationStatus::CanonicalMaster,
            canonical_master_id: Some(master_doc_id),
            canonical_master_path: Some(master_sidecar_path.clone()),
            drive_id: Some("primary".to_string()),
            drive_group: Some("primary".to_string()),
            is_in_master_path: true,
            reclaimable_bytes: 0,
            evaluated_at: chrono::Utc::now(),
            replication: None,
        }),
        visual: None,
    };

    // 3. Insert redundant source sidecar in unorganized with DIFFERENT hash (e.g. edit variant)
    let source_doc_id = Uuid::new_v4();
    let source_sidecar_path = unorganized_dir.join("IMG_8475.AAE");
    // Simulate source sidecar already removed by companion photo deletion
    let source_sidecar_doc = MediaMetadataDocument {
        id: source_doc_id,
        file_context: metadata_extractor::domain::metadata_models::FileContext {
            scanned_root: unorganized_dir.clone(),
            file_path: source_sidecar_path.clone(),
            relative_path: PathBuf::from("IMG_8475.AAE"),
            file_name: "IMG_8475.AAE".to_string(),
            file_size_bytes: 21,
            extension: "AAE".to_string(),
            media_category: MediaCategory::Sidecar,
            modified_at: None,
        },
        temporal: metadata_extractor::domain::metadata_models::TemporalMetadata::from_utc_datetime(
            chrono::Utc::now(),
            metadata_extractor::domain::metadata_models::TimestampSource::FileSystemModified,
        ),
        camera: None,
        exposure: None,
        location: None,
        dimensions: None,
        video: None,
        fingerprint: Some(metadata_extractor::domain::dedup_models::MediaFingerprint {
            id: Uuid::new_v4(),
            file_path: source_sidecar_path.clone(),
            file_size_bytes: 21,
            media_category: MediaCategory::Sidecar,
            dimensions: None,
            sample_digest: None,
            cryptographic_hash: "hash_source_variant_diff".to_string(),
            perceptual_hash: None,
            phash_hex: None,
            rotated_hashes: None,
            is_low_entropy: false,
            fingerprinted_at: chrono::Utc::now(),
        }),
        extraction_status: Default::default(),
        dedup: Some(DeduplicationMetadata {
            cluster_id: Uuid::new_v4(),
            status: DeduplicationStatus::MarkForRemoval,
            canonical_master_id: Some(Uuid::new_v4()), // Stale or different ID
            canonical_master_path: Some(master_sidecar_path.clone()),
            drive_id: Some("unorganized".to_string()),
            drive_group: None,
            is_in_master_path: false,
            reclaimable_bytes: 21,
            evaluated_at: chrono::Utc::now(),
            replication: None,
        }),
        visual: None,
    };

    sink.save_batch(&[master_sidecar_doc, source_sidecar_doc]).await.unwrap();

    let pruner = DrivePruningService::new();
    let report = pruner
        .prune_drive(
            &sink,
            &config,
            DrivePrunerOptions {
                drive_identifier: Some("unorganized".to_string()),
                execute: true,
                require_master_disk: true,
                manifest_out: None,
                ..Default::default()
            },
        )
        .await
        .expect("Prune should succeed");

    assert!(
        report.errors.is_empty(),
        "Expected no prune errors/warnings, got: {:?}",
        report.errors
    );
    assert_eq!(report.records_purged, 1, "Redundant AAE record must be purged from database");
    assert!(
        sink.find_document_by_id(source_doc_id).await.unwrap().is_none(),
        "Source AAE document must be deleted from sink"
    );
    assert!(
        sink.find_document_by_id(master_doc_id).await.unwrap().is_some(),
        "Master AAE document must remain untouched in sink"
    );
    assert!(master_sidecar_path.exists(), "Master sidecar on disk must remain untouched");
}
