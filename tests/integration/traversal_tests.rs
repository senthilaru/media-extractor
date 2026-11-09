use metadata_extractor::application::engine::ScanEngine;
use metadata_extractor::domain::models::MediaCategory;
use metadata_extractor::domain::ports::{DirectoryScanner, ScanConfig};
use std::path::PathBuf;

#[tokio::test]
async fn test_recursive_media_discovery() {
    let engine = ScanEngine::new();
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("media_samples");

    let config = ScanConfig {
        root_paths: vec![fixture_path],
        batch_size: 4,
        channel_capacity: 10,
        skip_hidden: true,
        follow_symlinks: false,
        ..Default::default()
    };

    let (_session, mut stream) = engine.start_scan(config).await.expect("scan should start");

    let mut discovered_items = Vec::new();
    while let Some(batch) = stream.batches.recv().await {
        discovered_items.extend(batch.items);
    }

    // Expected media files (skip_hidden = true):
    // photo1.jpg, photo2.PNG, video1.mp4, video2.MOV, raw1.CR3, raw2.dng, nested_photo.jpeg, nested_video.mkv
    // Excluded: notes.txt, .DS_Store, .hidden_dir/hidden_photo.jpg
    assert_eq!(discovered_items.len(), 8);

    // Verify categories
    let has_photo = discovered_items
        .iter()
        .any(|i| i.category == MediaCategory::Photo);
    let has_video = discovered_items
        .iter()
        .any(|i| i.category == MediaCategory::Video);
    let has_raw = discovered_items
        .iter()
        .any(|i| i.category == MediaCategory::RawPhoto);

    assert!(has_photo, "Should discover photo assets");
    assert!(has_video, "Should discover video assets");
    assert!(has_raw, "Should discover raw photo assets");

    // Verify non-media exclusion
    for item in &discovered_items {
        assert_ne!(item.extension, "txt");
        assert_ne!(item.extension, "DS_Store");
        assert!(item.file_size_bytes > 0);
    }
}

#[tokio::test]
async fn test_include_hidden_media() {
    let engine = ScanEngine::new();
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("media_samples");

    let config = ScanConfig {
        root_paths: vec![fixture_path],
        batch_size: 10,
        channel_capacity: 10,
        skip_hidden: false,
        follow_symlinks: false,
        ..Default::default()
    };

    let (_session, mut stream) = engine.start_scan(config).await.expect("scan should start");

    let mut discovered_items = Vec::new();
    while let Some(batch) = stream.batches.recv().await {
        discovered_items.extend(batch.items);
    }

    // With hidden files included, .hidden_dir/hidden_photo.jpg is also discovered -> 9 media files
    assert_eq!(discovered_items.len(), 9);
}
