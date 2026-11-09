use std::path::{Path, PathBuf};
use tempfile::tempdir;

use metadata_extractor::application::engine::ScanEngine;
use metadata_extractor::application::MetadataExtractionEngine;
use metadata_extractor::domain::metadata_models::MediaMetadataDocument;
use metadata_extractor::domain::models::{DiscoveredMediaItem, DiscoveryBatch, MediaCategory};
use metadata_extractor::domain::ports::{
    BatchMetadataExtractor, DirectoryScanner, MediaMetadataExtractor, ScanConfig,
};

#[tokio::test]
async fn test_metadata_pipeline_batch_extraction() {
    let dir = tempdir().expect("Failed to create tempdir");
    let base_path = dir.path();

    // Create a mock directory with photos and video files
    let photo1 = base_path.join("photo1.jpg");
    let photo2 = base_path.join("photo2.cr3");
    let video1 = base_path.join("clip1.mp4");

    std::fs::write(&photo1, b"mock jpeg data").unwrap();
    std::fs::write(&photo2, b"mock canon raw data").unwrap();
    std::fs::write(&video1, b"mock mp4 container data").unwrap();

    let scan_engine = ScanEngine::new();
    let config = ScanConfig {
        root_paths: vec![base_path.to_path_buf()],
        batch_size: 10,
        ..Default::default()
    };

    let (_session, mut stream) = scan_engine
        .start_scan(config)
        .await
        .expect("Scan should start");

    let metadata_engine = MetadataExtractionEngine::new();
    let mut total_docs = Vec::new();

    while let Some(batch) = stream.batches.recv().await {
        let docs = metadata_engine.extract_batch(batch, Some(base_path)).await;
        total_docs.extend(docs);
    }

    assert_eq!(total_docs.len(), 3);

    for doc in &total_docs {
        // Invariant: Non-empty UUID
        assert!(!doc.id.is_nil());

        // Invariant: Scanned root and relative path matching
        assert_eq!(doc.file_context.scanned_root, base_path);
        assert!(!doc.file_context.file_name.is_empty());

        // Invariant: Temporal integer facets populated
        assert!(doc.temporal.year >= 1970);
        assert!((1..=12).contains(&doc.temporal.month));
        assert!((1..=31).contains(&doc.temporal.day));
        assert!((0..=23).contains(&doc.temporal.hour));
        assert!((0..=59).contains(&doc.temporal.minute));
        assert!((0..=59).contains(&doc.temporal.second));

        // Invariant: Fingerprint populated
        assert!(doc.fingerprint.is_some());
        let fp = doc.fingerprint.as_ref().unwrap();
        assert_eq!(fp.cryptographic_hash.len(), 64);
        assert!(fp.sample_digest.is_some());

        // Invariant: Lossless roundtrip JSON serialization (MongoDB ready)
        let json_str = serde_json::to_string(doc).expect("Serialization to JSON must succeed");
        let roundtrip: MediaMetadataDocument =
            serde_json::from_str(&json_str).expect("Deserialization from JSON must succeed");
        assert_eq!(doc.id, roundtrip.id);
        assert_eq!(doc.file_context.file_name, roundtrip.file_context.file_name);
        assert_eq!(doc.temporal.year, roundtrip.temporal.year);
        assert_eq!(
            doc.fingerprint.as_ref().unwrap().cryptographic_hash,
            roundtrip.fingerprint.as_ref().unwrap().cryptographic_hash
        );
    }
}

#[tokio::test]
async fn test_corrupt_file_pipeline_resilience() {
    let metadata_engine = MetadataExtractionEngine::new();
    let corrupt_path = PathBuf::from("tests/fixtures/corrupt_samples/corrupt_exif.jpg");

    let item = DiscoveredMediaItem {
        path: corrupt_path.clone(),
        file_size_bytes: 20,
        modified_at: None,
        category: MediaCategory::Photo,
        extension: "jpg".to_string(),
    };

    let batch = DiscoveryBatch {
        session_id: uuid::Uuid::new_v4(),
        batch_sequence: 1,
        items: vec![item],
        count: 1,
        created_at: std::time::SystemTime::now(),
    };

    let docs = metadata_engine
        .extract_batch(batch, Some(Path::new("tests/fixtures")))
        .await;

    assert_eq!(docs.len(), 1);
    let doc = &docs[0];
    assert!(!doc.id.is_nil());
    assert_eq!(doc.file_context.extension, "jpg");
    assert!(
        !doc.extraction_status.warnings.is_empty() || doc.extraction_status.error_message.is_some()
    );
}

#[test]
fn test_metadata_pipeline_fingerprint_extraction() {
    let engine = MetadataExtractionEngine::new();
    let photo_path = PathBuf::from("tests/fixtures/valid_samples/valid_exif.jpg");

    let item = DiscoveredMediaItem {
        path: photo_path.clone(),
        file_size_bytes: 1260,
        modified_at: None,
        category: MediaCategory::Photo,
        extension: "jpg".to_string(),
    };

    let doc = engine
        .extract_file(&item, Some(Path::new("tests/fixtures")))
        .expect("Extraction must succeed");

    assert!(
        doc.fingerprint.is_some(),
        "Document must include fingerprint"
    );
    let fp = doc.fingerprint.unwrap();

    assert_eq!(fp.cryptographic_hash.len(), 64);
    assert!(fp.sample_digest.is_some());
    assert!(fp.perceptual_hash.is_some());
    assert!(fp.phash_hex.is_some());
    assert!(fp.rotated_hashes.is_some());
    assert!(!fp.is_low_entropy);
}
