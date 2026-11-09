use std::fs::File;
use std::io::Write;
use tempfile::tempdir;

use metadata_extractor::application::engine::ScanEngine;
use metadata_extractor::domain::ports::{DirectoryScanner, ScanConfig};

#[tokio::test]
async fn test_backpressure_and_throttled_consumer() {
    let dir = tempdir().expect("temp dir");
    let base_path = dir.path().to_path_buf();

    // Create 30 fake images
    for i in 1..=30 {
        let file_path = base_path.join(format!("image_{:03}.jpg", i));
        let mut file = File::create(&file_path).expect("create file");
        writeln!(file, "dummy content for photo {}", i).expect("write content");
    }

    let engine = ScanEngine::new();
    let config = ScanConfig {
        root_paths: vec![base_path],
        batch_size: 5,
        channel_capacity: 2, // Only buffer 2 batches max
        skip_hidden: true,
        follow_symlinks: false,
        ..Default::default()
    };

    let (_session, mut stream) = engine.start_scan(config).await.expect("start scan");

    let mut total_received = 0usize;
    let mut batch_count = 0usize;

    // Simulate throttled consumer reading with small delays
    while let Some(batch) = stream.batches.recv().await {
        batch_count += 1;
        total_received += batch.count;
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
    }

    assert_eq!(total_received, 30);
    assert_eq!(batch_count, 6); // 30 items / 5 per batch = 6 batches
}

#[tokio::test]
async fn test_scan_cancellation() {
    let dir = tempdir().expect("temp dir");
    let base_path = dir.path().to_path_buf();

    // Create 200 files across multiple subfolders
    for folder in 0..10 {
        let sub = base_path.join(format!("sub_{}", folder));
        std::fs::create_dir_all(&sub).expect("create dir");
        for i in 1..=20 {
            let file_path = sub.join(format!("image_{:03}.jpg", i));
            let mut file = File::create(&file_path).expect("create file");
            writeln!(file, "dummy content {}", i).expect("write content");
        }
    }

    let engine = ScanEngine::new();
    let config = ScanConfig {
        root_paths: vec![base_path],
        batch_size: 5,
        channel_capacity: 1, // Bounded to 1 batch in flight
        ..Default::default()
    };

    let (session, mut stream) = engine.start_scan(config).await.expect("start scan");

    // Read 1 batch then request cancellation immediately
    let first_batch = stream.batches.recv().await;
    assert!(first_batch.is_some());

    engine
        .cancel_scan(&session.session_id)
        .await
        .expect("cancel scan");

    // Drain remaining in-flight batches
    let mut remaining = 0;
    while stream.batches.recv().await.is_some() {
        remaining += 1;
    }

    // Traversal should halt without generating all 40 batches (200 files / 5)
    assert!(
        remaining < 30,
        "Expected traversal to halt early, but got {} batches",
        remaining
    );
}
