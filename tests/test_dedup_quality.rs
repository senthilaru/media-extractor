use chrono::Utc;
use metadata_extractor::application::dedup_engine::MediaDeduplicationEngine;
use metadata_extractor::domain::dedup_models::{
    DeduplicationStatus, DriveConfiguration, DriveDefinition, DriveGroupSpec, DriveRules,
    MediaFingerprint,
};
use metadata_extractor::domain::metadata_models::{
    ExtractionStatus, FileContext, ImageDimensions, MediaMetadataDocument, TemporalMetadata,
    TimestampSource,
};
use metadata_extractor::domain::models::MediaCategory;
use metadata_extractor::infrastructure::drives_config::CanonicalPathMatcher;
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

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
    groups.insert(
        "staging".to_string(),
        DriveGroupSpec {
            priority: 20,
            description: Some("Staging / Desktop".to_string()),
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
        DriveDefinition {
            id: "staging_desktop".to_string(),
            group: "staging".to_string(),
            mount_path: PathBuf::from("/Users/spsenthil/Desktop"),
            master_path: PathBuf::from("/Users/spsenthil/Desktop/photos"),
        },
    ];

    DriveConfiguration {
        version: "1.0".to_string(),
        groups,
        drives,
        rules: DriveRules::default(),
    }
}

fn create_mock_doc_with_dims(
    path_str: &str,
    size_bytes: u64,
    dims: Option<(u32, u32)>,
    hash: &str,
) -> MediaMetadataDocument {
    let path = PathBuf::from(path_str);
    let now = Utc::now();
    let filename = path.file_name().unwrap().to_string_lossy().to_string();
    let image_dims = dims.map(|(w, h)| ImageDimensions {
        width: w,
        height: h,
        orientation: None,
    });

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
        dimensions: image_dims,
        video: None,
        fingerprint: Some(MediaFingerprint {
            id: Uuid::new_v4(),
            file_path: path,
            file_size_bytes: size_bytes,
            media_category: MediaCategory::Photo,
            dimensions: image_dims,
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
        visual: None,
    }
}

#[test]
fn test_us1_mixed_resolution_cluster_selects_highest_resolution() {
    let config = create_test_config();
    let matcher = CanonicalPathMatcher::new(config);

    let doc_4k = create_mock_doc_with_dims(
        "/Users/spsenthil/Desktop/photos/image_4k.jpg",
        4_000_000,
        Some((4000, 3000)), // 12 MP
        "hash_cluster_1",
    );
    let doc_1080p = create_mock_doc_with_dims(
        "/Users/spsenthil/Desktop/photos/image_1080p.jpg",
        2_000_000,
        Some((1920, 1080)), // 2 MP
        "hash_cluster_1",
    );
    let doc_thumb = create_mock_doc_with_dims(
        "/Users/spsenthil/Desktop/photos/image_thumb.jpg",
        50_000,
        Some((300, 300)), // 0.09 MP
        "hash_cluster_1",
    );

    let id_4k = doc_4k.id;
    let id_1080p = doc_1080p.id;
    let id_thumb = doc_thumb.id;

    let docs = vec![doc_thumb, doc_4k, doc_1080p]; // Intentionally shuffled order
    let evaluations = MediaDeduplicationEngine::evaluate_content_group(&docs, &matcher);

    let eval_map: HashMap<Uuid, DeduplicationStatus> =
        evaluations.into_iter().map(|(id, meta)| (id, meta.status)).collect();

    assert_eq!(
        eval_map.get(&id_4k),
        Some(&DeduplicationStatus::NeedsIngestToMaster),
        "Highest resolution (4K) must be selected for ingest to master"
    );
    assert_eq!(
        eval_map.get(&id_1080p),
        Some(&DeduplicationStatus::MarkForRemoval),
        "Lower resolution (1080p) copy must be marked for removal"
    );
    assert_eq!(
        eval_map.get(&id_thumb),
        Some(&DeduplicationStatus::MarkForRemoval),
        "Lowest resolution thumbnail must be marked for removal"
    );
}

#[test]
fn test_us2_equal_resolution_selects_larger_file_size() {
    let config = create_test_config();
    let matcher = CanonicalPathMatcher::new(config);

    let doc_high_fidelity = create_mock_doc_with_dims(
        "/Users/spsenthil/Desktop/photos/high_fidelity.jpg",
        5_000_000, // 5 MB
        Some((1920, 1080)),
        "hash_cluster_2",
    );
    let doc_compressed = create_mock_doc_with_dims(
        "/Users/spsenthil/Desktop/photos/compressed.jpg",
        450_000, // 450 KB
        Some((1920, 1080)),
        "hash_cluster_2",
    );

    let id_high_fidelity = doc_high_fidelity.id;
    let id_compressed = doc_compressed.id;

    let docs = vec![doc_compressed, doc_high_fidelity];
    let evaluations = MediaDeduplicationEngine::evaluate_content_group(&docs, &matcher);

    let eval_map: HashMap<Uuid, DeduplicationStatus> =
        evaluations.into_iter().map(|(id, meta)| (id, meta.status)).collect();

    assert_eq!(
        eval_map.get(&id_high_fidelity),
        Some(&DeduplicationStatus::NeedsIngestToMaster),
        "Larger file size must be preserved as keeper when resolutions match"
    );
    assert_eq!(
        eval_map.get(&id_compressed),
        Some(&DeduplicationStatus::MarkForRemoval),
        "Smaller, heavily compressed file must be marked for removal"
    );
}

#[test]
fn test_us3_master_upgrade_with_higher_resolution_external_candidate() {
    let config = create_test_config();
    let matcher = CanonicalPathMatcher::new(config);

    // Existing low-res copy on primary master
    let doc_master_low_res = create_mock_doc_with_dims(
        "/Volumes/SP-1/SPSA/media/master/2024/01-Jan-2024/Photos/photo.jpg",
        300_000,
        Some((1024, 768)), // 0.78 MP
        "hash_cluster_3",
    );

    // Superior full-resolution copy on staging drive
    let doc_staging_high_res = create_mock_doc_with_dims(
        "/Users/spsenthil/Desktop/photos/photo_fullres.jpg",
        8_000_000,
        Some((4032, 3024)), // 12 MP
        "hash_cluster_3",
    );

    // Redundant preview copy on staging drive
    let doc_staging_preview = create_mock_doc_with_dims(
        "/Users/spsenthil/Desktop/photos/photo_thumb.jpg",
        40_000,
        Some((320, 240)),
        "hash_cluster_3",
    );

    let id_master = doc_master_low_res.id;
    let id_high_res = doc_staging_high_res.id;
    let id_preview = doc_staging_preview.id;

    let docs = vec![doc_staging_preview, doc_master_low_res, doc_staging_high_res];
    let evaluations = MediaDeduplicationEngine::evaluate_content_group(&docs, &matcher);

    let eval_map: HashMap<Uuid, DeduplicationStatus> =
        evaluations.into_iter().map(|(id, meta)| (id, meta.status)).collect();

    assert_eq!(
        eval_map.get(&id_high_res),
        Some(&DeduplicationStatus::NeedsIngestToMaster),
        "Superior high-res external candidate must be marked NeedsIngestToMaster to upgrade master"
    );
    assert_eq!(
        eval_map.get(&id_master),
        Some(&DeduplicationStatus::CanonicalMaster),
        "Existing master file remains CanonicalMaster until superior copy is ingested"
    );
    assert_eq!(
        eval_map.get(&id_preview),
        Some(&DeduplicationStatus::MarkForRemoval),
        "Inferior preview must be marked for removal"
    );
}
