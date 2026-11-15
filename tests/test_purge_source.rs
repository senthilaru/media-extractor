use async_trait::async_trait;
use chrono::Utc;
use std::collections::HashMap;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tempfile::tempdir;
use uuid::Uuid;

use metadata_extractor::application::{PurgeOptions, PurgeService};
use metadata_extractor::domain::dedup_models::{
    DeduplicationMetadata, DeduplicationStatus, DriveConfiguration, DriveDefinition, DriveGroupSpec,
    DriveRules, MediaFingerprint,
};
use metadata_extractor::domain::errors::StorageError;
use metadata_extractor::domain::metadata_models::{
    ExtractionStatus, FileContext, FilterEvaluationResult, MediaMetadataDocument, TemporalMetadata,
    TimestampSource,
};
use metadata_extractor::domain::models::{DiscoveredMediaItem, MediaCategory};
use metadata_extractor::domain::ports::{BatchWriteStats, MetadataStorageSink};

struct MockPurgeStorageSink {
    docs: Vec<MediaMetadataDocument>,
    purged_count: AtomicUsize,
}

#[async_trait]
impl MetadataStorageSink for MockPurgeStorageSink {
    async fn ping(&self) -> Result<(), StorageError> {
        Ok(())
    }

    async fn ensure_indexes(&self) -> Result<(), StorageError> {
        Ok(())
    }

    async fn save_batch(
        &self,
        _docs: &[MediaMetadataDocument],
    ) -> Result<BatchWriteStats, StorageError> {
        Ok(BatchWriteStats {
            matched_count: 0,
            modified_count: 0,
            upserted_count: 0,
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

    async fn delete_document(&self, _doc_id: Uuid) -> Result<bool, StorageError> {
        Ok(false)
    }

    async fn delete_documents(&self, doc_ids: &[Uuid]) -> Result<usize, StorageError> {
        Ok(doc_ids.len())
    }

    async fn purge_removed_records(
        &self,
        _mount_prefix: Option<&str>,
    ) -> Result<usize, StorageError> {
        Ok(0)
    }

    async fn query_all_content_groups(
        &self,
        _mount_prefix: Option<&str>,
    ) -> Result<Vec<(String, Vec<MediaMetadataDocument>)>, StorageError> {
        Ok(Vec::new())
    }

    async fn query_canonical_masters(&self) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        Ok(Vec::new())
    }

    async fn query_replica_documents(
        &self,
        _replica_drive_id: &str,
    ) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        Ok(Vec::new())
    }

    async fn query_by_source_path_prefix(
        &self,
        prefix: &str,
    ) -> Result<Vec<MediaMetadataDocument>, StorageError> {
        let matched = self
            .docs
            .iter()
            .filter(|d| {
                d.file_context.file_path.to_string_lossy().starts_with(prefix)
                    || d.fingerprint
                        .as_ref()
                        .map(|fp| fp.file_path.to_string_lossy().starts_with(prefix))
                        .unwrap_or(false)
            })
            .cloned()
            .collect();
        Ok(matched)
    }

    async fn purge_documents_by_ids(&self, doc_ids: &[Uuid]) -> Result<usize, StorageError> {
        self.purged_count.fetch_add(doc_ids.len(), Ordering::SeqCst);
        Ok(doc_ids.len())
    }
}

fn create_sample_doc(
    id: Uuid,
    file_path: PathBuf,
    source_path: PathBuf,
    status: DeduplicationStatus,
    canonical_master_path: Option<PathBuf>,
) -> MediaMetadataDocument {
    let now = Utc::now();
    let filename = file_path.file_name().unwrap().to_string_lossy().to_string();

    MediaMetadataDocument {
        id,
        file_context: FileContext {
            scanned_root: PathBuf::from("/"),
            file_path,
            relative_path: PathBuf::from(&filename),
            file_name: filename,
            file_size_bytes: 1024,
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
            file_path: source_path,
            file_size_bytes: 1024,
            media_category: MediaCategory::Photo,
            dimensions: None,
            sample_digest: Some("sample".to_string()),
            cryptographic_hash: "samplehash".to_string(),
            perceptual_hash: None,
            phash_hex: None,
            rotated_hashes: None,
            is_low_entropy: false,
            fingerprinted_at: now,
        }),
        extraction_status: ExtractionStatus::default(),
        dedup: Some(DeduplicationMetadata {
            cluster_id: Uuid::new_v4(),
            status,
            canonical_master_id: None,
            canonical_master_path,
            drive_id: Some("primary".to_string()),
            drive_group: Some("primary".to_string()),
            is_in_master_path: status == DeduplicationStatus::CanonicalMaster,
            reclaimable_bytes: 0,
            evaluated_at: now,
            replication: None,
        }),
    }
}

#[tokio::test]
async fn test_purge_source_segregates_paths_and_generates_manifest() {
    let temp = tempdir().unwrap();
    let primary_mount = temp.path().join("primary_drive");
    let primary_master = primary_mount.join("media/master");
    let replica_mount = temp.path().join("replica_drive");
    let replica_master = replica_mount.join("media/master");
    let source_folder = temp.path().join("downloads/unwanted_folder");

    std::fs::create_dir_all(&primary_master).unwrap();
    std::fs::create_dir_all(&replica_master).unwrap();
    std::fs::create_dir_all(&source_folder).unwrap();

    // Create physical files
    let master_file = primary_master.join("photo_master.jpg");
    File::create(&master_file).unwrap().write_all(b"master-bytes").unwrap();

    let replica_file = replica_master.join("photo_master.jpg");
    File::create(&replica_file).unwrap().write_all(b"master-bytes").unwrap();

    let source_file = source_folder.join("photo.jpg");
    File::create(&source_file).unwrap().write_all(b"source-bytes").unwrap();

    let mut groups = HashMap::new();
    groups.insert(
        "primary".to_string(),
        DriveGroupSpec {
            priority: 100,
            description: Some("primary".to_string()),
        },
    );
    groups.insert(
        "replica".to_string(),
        DriveGroupSpec {
            priority: 80,
            description: Some("replica".to_string()),
        },
    );

    let config = DriveConfiguration {
        version: "1.0".to_string(),
        groups,
        rules: DriveRules::default(),
        drives: vec![
            DriveDefinition {
                id: "primary".to_string(),
                group: "primary".to_string(),
                mount_path: primary_mount.clone(),
                master_path: primary_master.clone(),
            },
            DriveDefinition {
                id: "replica1".to_string(),
                group: "replica".to_string(),
                mount_path: replica_mount.clone(),
                master_path: replica_master.clone(),
            },
        ],
    };

    let doc_master = create_sample_doc(
        Uuid::new_v4(),
        master_file.clone(),
        source_file.clone(),
        DeduplicationStatus::CanonicalMaster,
        Some(master_file.clone()),
    );

    let doc_source = create_sample_doc(
        Uuid::new_v4(),
        source_file.clone(),
        source_file.clone(),
        DeduplicationStatus::MarkForRemoval,
        Some(master_file.clone()),
    );

    let sink = Arc::new(MockPurgeStorageSink {
        docs: vec![doc_master, doc_source],
        purged_count: AtomicUsize::new(0),
    });

    let service = PurgeService::new(sink.clone(), config);
    let manifest_path = temp.path().join("test_manifest.txt");

    // 1. Dry run test
    let dry_run_opts = PurgeOptions {
        execute: false,
        delete_source: false,
        manifest_output_path: manifest_path.clone(),
    };

    let report = service.purge_source(&source_folder, &dry_run_opts).await.unwrap();
    assert_eq!(report.total_matched_records, 2);
    assert_eq!(report.records_purged_from_db, 0);
    assert_eq!(sink.purged_count.load(Ordering::SeqCst), 0);
    assert!(manifest_path.exists());

    let manifest_content = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(manifest_content.contains("MANUAL MEDIA PURGE MANIFEST"));
    assert!(manifest_content.contains(&master_file.to_string_lossy().to_string()));
    assert!(manifest_content.contains(&source_file.to_string_lossy().to_string()));

    // Master file and source file must still exist
    assert!(master_file.exists());
    assert!(source_file.exists());

    // 2. Execute test with delete_source
    let exec_opts = PurgeOptions {
        execute: true,
        delete_source: true,
        manifest_output_path: manifest_path.clone(),
    };

    let report_exec = service.purge_source(&source_folder, &exec_opts).await.unwrap();
    assert_eq!(report_exec.records_purged_from_db, 2);
    assert_eq!(sink.purged_count.load(Ordering::SeqCst), 2);

    // Source file deleted on disk, but Master file is STRICTLY PROTECTED
    assert!(!source_file.exists(), "Source file should have been deleted");
    assert!(master_file.exists(), "Master file MUST NEVER be automatically deleted!");
}
