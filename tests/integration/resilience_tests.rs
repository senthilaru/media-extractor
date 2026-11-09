use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use tempfile::tempdir;

use metadata_extractor::application::engine::ScanEngine;
use metadata_extractor::domain::errors::{ScanError, ScanErrorKind};
use metadata_extractor::domain::ports::{DirectoryScanner, ScanConfig};

#[tokio::test]
async fn test_circular_symlink_detection() {
    let dir = tempdir().expect("temp dir");
    let base_path = dir.path().to_path_buf();

    // Create a regular photo file
    let valid_photo = base_path.join("normal.jpg");
    let mut file = File::create(&valid_photo).expect("create file");
    writeln!(file, "valid photo").expect("write");

    // Create a subdirectory with a circular symlink back to base_path
    let sub = base_path.join("subfolder");
    std::fs::create_dir_all(&sub).expect("create sub");

    #[cfg(unix)]
    {
        let loop_link = sub.join("loop_to_parent");
        let _ = std::os::unix::fs::symlink(&base_path, &loop_link);
    }

    let engine = ScanEngine::new();
    let config = ScanConfig {
        root_paths: vec![base_path],
        batch_size: 10,
        channel_capacity: 10,
        skip_hidden: false,
        follow_symlinks: true,
        ..Default::default()
    };

    let (_session, mut stream) = engine.start_scan(config).await.expect("start scan");

    let mut discovered = Vec::new();
    while let Some(batch) = stream.batches.recv().await {
        discovered.extend(batch.items);
    }

    let mut errors = Vec::new();
    while let Ok(err) = stream.errors.try_recv() {
        errors.push(err);
    }

    // Normal photo must be found
    assert!(!discovered.is_empty());
    assert!(discovered.iter().any(|i| i.path.ends_with("normal.jpg")));

    // At least one circular symlink error should be captured on unix
    #[cfg(unix)]
    {
        let has_cycle = errors
            .iter()
            .any(|e| e.kind == ScanErrorKind::CircularSymlink);
        assert!(has_cycle, "Expected CircularSymlink error to be caught");
    }
}

#[tokio::test]
async fn test_invalid_path_error() {
    let engine = ScanEngine::new();
    let config = ScanConfig {
        root_paths: vec![PathBuf::from("/nonexistent/invalid/directory/path/12345")],
        ..Default::default()
    };

    let result = engine.start_scan(config).await;
    assert!(matches!(result, Err(ScanError::InvalidPath(_))));
}
