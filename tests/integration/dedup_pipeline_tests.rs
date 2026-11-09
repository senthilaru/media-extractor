use std::path::PathBuf;

use metadata_extractor::domain::dedup_models::{
    DeduplicationConfig, DuplicateCluster, DuplicateRelationship, FingerprintConfig,
};
use metadata_extractor::domain::models::{DiscoveredMediaItem, MediaCategory};
use metadata_extractor::domain::ports::{DeduplicationEngine, MediaFingerprinter};
use metadata_extractor::infrastructure::blake3_hasher::Blake3MediaHasher;
use metadata_extractor::MediaDeduplicationEngine;

fn fixture_path(filename: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("dedup_samples")
        .join(filename)
}

fn discovered_item(path: PathBuf) -> DiscoveredMediaItem {
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(1000);
    DiscoveredMediaItem {
        path,
        file_size_bytes: size,
        modified_at: None,
        category: MediaCategory::Photo,
        extension: "jpg".to_string(),
    }
}

#[tokio::test]
async fn test_exact_duplicate_clustering() {
    let hasher = Blake3MediaHasher::new();
    let fp_config = FingerprintConfig::default();
    let dedup_config = DeduplicationConfig::default();

    let p1 = fixture_path("photo_original.jpg");
    let p2 = fixture_path("photo_exact_copy.jpg");

    let fp1 = hasher
        .fingerprint_file(&discovered_item(p1), None, &fp_config)
        .expect("fp1");
    let fp2 = hasher
        .fingerprint_file(&discovered_item(p2), None, &fp_config)
        .expect("fp2");

    let engine = MediaDeduplicationEngine::new();
    let (clusters, summary) = engine
        .find_duplicates(vec![fp1, fp2], dedup_config)
        .await
        .expect("dedup should succeed");

    assert_eq!(clusters.len(), 1);
    let cluster = &clusters[0];
    assert_eq!(
        cluster.relationship_type,
        DuplicateRelationship::ExactBitwise
    );
    assert_eq!(cluster.variants.len(), 1);
    assert_eq!(cluster.variants[0].similarity_score, 1.0);
    assert_eq!(summary.exact_duplicate_files, 1);
    assert_eq!(summary.duplicate_clusters_count, 1);
    assert!(summary.total_reclaimable_bytes > 0);
}

#[tokio::test]
async fn test_perceptual_thumbnail_clustering() {
    let hasher = Blake3MediaHasher::new();
    let fp_config = FingerprintConfig {
        enable_perceptual: true,
        enable_rotation: true,
        ..Default::default()
    };
    let dedup_config = DeduplicationConfig {
        similarity_threshold: 10,
        enable_perceptual: true,
        min_cluster_size: 2,
    };

    let p_orig = fixture_path("photo_original.jpg");
    let p_thumb = fixture_path("photo_thumbnail.jpg");

    let fp_orig = hasher
        .fingerprint_file(&discovered_item(p_orig.clone()), None, &fp_config)
        .expect("fp_orig");
    let fp_thumb = hasher
        .fingerprint_file(&discovered_item(p_thumb.clone()), None, &fp_config)
        .expect("fp_thumb");

    let engine = MediaDeduplicationEngine::new();
    let (clusters, summary) = engine
        .find_duplicates(vec![fp_orig, fp_thumb], dedup_config)
        .await
        .expect("dedup should succeed");

    assert_eq!(clusters.len(), 1);
    let cluster = &clusters[0];
    // Master must be photo_original (higher resolution/quality)
    assert_eq!(cluster.canonical_asset.file_path, p_orig);
    assert_eq!(cluster.variants.len(), 1);
    assert_eq!(cluster.variants[0].file_path, p_thumb);
    assert_eq!(summary.near_duplicate_files, 1);
}

#[tokio::test]
async fn test_low_entropy_images_not_clustered() {
    let hasher = Blake3MediaHasher::new();
    let fp_config = FingerprintConfig {
        enable_perceptual: true,
        enable_rotation: true,
        ..Default::default()
    };
    let dedup_config = DeduplicationConfig {
        similarity_threshold: 10,
        enable_perceptual: true,
        min_cluster_size: 2,
    };

    let p_black = fixture_path("blank_black.jpg");
    let p_white = fixture_path("blank_white.jpg");

    let fp_black = hasher
        .fingerprint_file(&discovered_item(p_black), None, &fp_config)
        .expect("fp_black");
    let fp_white = hasher
        .fingerprint_file(&discovered_item(p_white), None, &fp_config)
        .expect("fp_white");

    let engine = MediaDeduplicationEngine::new();
    let (clusters, _) = engine
        .find_duplicates(vec![fp_black, fp_white], dedup_config)
        .await
        .expect("dedup should succeed");

    // Must NOT cluster low-entropy blank images together
    assert_eq!(clusters.len(), 0);
}

#[test]
fn test_cluster_json_serialization() {
    let cluster = DuplicateCluster {
        cluster_id: uuid::Uuid::new_v4(),
        relationship_type: DuplicateRelationship::ExactBitwise,
        canonical_asset: metadata_extractor::CanonicalAsset {
            file_path: PathBuf::from("/photos/original.jpg"),
            file_size_bytes: 2048,
            dimensions: None,
            media_category: MediaCategory::Photo,
            quality_score: 200.0,
        },
        variants: vec![metadata_extractor::DuplicateVariant {
            file_path: PathBuf::from("/photos/copy.jpg"),
            file_size_bytes: 2048,
            relationship: DuplicateRelationship::ExactBitwise,
            hamming_distance: Some(0),
            similarity_score: 1.0,
            reclaimable_bytes: 2048,
        }],
        total_reclaimable_bytes: 2048,
        total_files: 2,
    };

    let json_str = serde_json::to_string(&cluster).expect("serialization to json");
    let deserialized: DuplicateCluster =
        serde_json::from_str(&json_str).expect("deserialization from json");

    assert_eq!(cluster.cluster_id, deserialized.cluster_id);
    assert_eq!(
        cluster.total_reclaimable_bytes,
        deserialized.total_reclaimable_bytes
    );
}
