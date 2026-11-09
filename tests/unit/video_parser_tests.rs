use std::path::PathBuf;

use metadata_extractor::domain::errors::ExtractionError;
use metadata_extractor::domain::metadata_models::{ExtractionResultKind, TimestampSource};
use metadata_extractor::domain::models::{DiscoveredMediaItem, MediaCategory};
use metadata_extractor::domain::ports::MediaMetadataExtractor;
use metadata_extractor::infrastructure::video_parser::VideoMetadataExtractor;

#[test]
fn test_video_extract_file_not_found() {
    let extractor = VideoMetadataExtractor::new();
    let missing_item = DiscoveredMediaItem {
        path: PathBuf::from("/non/existent/clip.mp4"),
        file_size_bytes: 500,
        modified_at: None,
        category: MediaCategory::Video,
        extension: "mp4".to_string(),
    };

    let result = extractor.extract_file(&missing_item, None);
    assert!(result.is_err());
    match result.unwrap_err() {
        ExtractionError::FileNotFound(path) => {
            assert_eq!(path, PathBuf::from("/non/existent/clip.mp4"));
        }
        other => panic!("Expected FileNotFound, got {:?}", other),
    }
}

#[test]
fn test_video_extract_plain_container_filesystem_fallback() {
    let temp_file = tempfile::Builder::new()
        .suffix(".mp4")
        .tempfile()
        .expect("Failed to create temp file");
    let temp_path = temp_file.path().to_path_buf();

    let extractor = VideoMetadataExtractor::new();
    let item = DiscoveredMediaItem {
        path: temp_path.clone(),
        file_size_bytes: 0,
        modified_at: None,
        category: MediaCategory::Video,
        extension: "mp4".to_string(),
    };

    let doc = extractor
        .extract_file(&item, temp_path.parent())
        .expect("Extraction should succeed with filesystem fallback");

    assert_eq!(doc.temporal.source, TimestampSource::FileSystemModified);
    assert_eq!(doc.file_context.media_category, MediaCategory::Video);
    assert_eq!(doc.file_context.extension, "mp4");
    assert_eq!(doc.extraction_status.status, ExtractionResultKind::Partial);
    assert!(!doc.extraction_status.warnings.is_empty());
}

#[test]
fn test_extract_valid_video_container() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = PathBuf::from(manifest_dir).join("tests/fixtures/valid_samples/valid_video.mp4");
    assert!(path.exists(), "valid_video.mp4 fixture must exist");

    let extractor = VideoMetadataExtractor::new();
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let item = DiscoveredMediaItem {
        path: path.clone(),
        file_size_bytes: size,
        modified_at: None,
        category: MediaCategory::Video,
        extension: "mp4".to_string(),
    };

    let doc = extractor
        .extract_file(&item, Some(std::path::Path::new(manifest_dir)))
        .expect("Extraction from valid video container must succeed");

    assert_eq!(doc.file_context.extension, "mp4");
    assert_eq!(doc.file_context.media_category, MediaCategory::Video);

    let dims = doc.dimensions.expect("Video dimensions should be present");
    assert_eq!(dims.width, 1920);
    assert_eq!(dims.height, 1080);

    let vid = doc.video.expect("Video metadata should be present");
    assert_eq!(vid.duration_ms, 5000);
    assert_eq!(vid.video_codec.as_deref(), Some("h264"));
    assert_eq!(vid.frame_rate_fps, Some(30.0));
    assert_eq!(vid.rotation_degrees, Some(0));
}

#[test]
fn test_extract_rotated_video_container() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = PathBuf::from(manifest_dir).join("tests/fixtures/valid_samples/rotated_video.mp4");
    assert!(path.exists(), "rotated_video.mp4 fixture must exist");

    let extractor = VideoMetadataExtractor::new();
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let item = DiscoveredMediaItem {
        path: path.clone(),
        file_size_bytes: size,
        modified_at: None,
        category: MediaCategory::Video,
        extension: "mp4".to_string(),
    };

    let doc = extractor
        .extract_file(&item, Some(std::path::Path::new(manifest_dir)))
        .expect("Extraction from rotated video container must succeed");

    let dims = doc
        .dimensions
        .expect("Rotated video display dimensions should be present");
    // Effective display dimensions: 1920x1080 rotated 90 degrees becomes 1080x1920
    assert_eq!(dims.width, 1080);
    assert_eq!(dims.height, 1920);

    let vid = doc.video.expect("Video metadata should be present");
    assert_eq!(vid.rotation_degrees, Some(90));
    assert_eq!(vid.video_codec.as_deref(), Some("h264"));
    assert_eq!(vid.frame_rate_fps, Some(30.0));
}

#[test]
fn test_extract_tail_moov_video_container() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = PathBuf::from(manifest_dir).join("tests/fixtures/valid_samples/tail_moov_video.mp4");
    assert!(path.exists(), "tail_moov_video.mp4 fixture must exist");

    let extractor = VideoMetadataExtractor::new();
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let item = DiscoveredMediaItem {
        path: path.clone(),
        file_size_bytes: size,
        modified_at: None,
        category: MediaCategory::Video,
        extension: "mp4".to_string(),
    };

    let doc = extractor
        .extract_file(&item, Some(std::path::Path::new(manifest_dir)))
        .expect("Extraction from tail moov video must succeed");

    let dims = doc.dimensions.expect("Video dimensions should be present");
    assert_eq!(dims.width, 1920);
    assert_eq!(dims.height, 1080);

    let vid = doc.video.expect("Video metadata should be present");
    assert_eq!(vid.duration_ms, 5000);
}
