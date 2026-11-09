use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

use metadata_extractor::domain::errors::ExtractionError;
use metadata_extractor::domain::metadata_models::{
    ExtractionResultKind, GeoLocation, TimestampSource,
};
use metadata_extractor::domain::models::{DiscoveredMediaItem, MediaCategory};
use metadata_extractor::domain::ports::MediaMetadataExtractor;
use metadata_extractor::infrastructure::exif_parser::ExifMetadataExtractor;

#[test]
fn test_extract_file_not_found() {
    let extractor = ExifMetadataExtractor::new();
    let missing_item = DiscoveredMediaItem {
        path: PathBuf::from("/non/existent/path/photo.jpg"),
        file_size_bytes: 100,
        modified_at: None,
        category: MediaCategory::Photo,
        extension: "jpg".to_string(),
    };

    let result = extractor.extract_file(&missing_item, None);
    assert!(result.is_err());
    match result.unwrap_err() {
        ExtractionError::FileNotFound(path) => {
            assert_eq!(path, PathBuf::from("/non/existent/path/photo.jpg"));
        }
        other => panic!("Expected FileNotFound, got {:?}", other),
    }
}

#[test]
fn test_extract_corrupt_exif_graceful_fallback() {
    let extractor = ExifMetadataExtractor::new();
    let corrupt_path = PathBuf::from("tests/fixtures/corrupt_samples/corrupt_exif.jpg");

    if !corrupt_path.exists() {
        // If relative execution path differs, construct from CARGO_MANIFEST_DIR
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let path = PathBuf::from(manifest_dir).join(&corrupt_path);
        let item = DiscoveredMediaItem {
            path,
            file_size_bytes: 20,
            modified_at: None,
            category: MediaCategory::Photo,
            extension: "jpg".to_string(),
        };
        let doc = extractor
            .extract_file(&item, Some(Path::new(manifest_dir)))
            .expect("Corrupt EXIF should not crash extractor");
        assert_eq!(doc.extraction_status.status, ExtractionResultKind::Partial);
        assert_eq!(doc.temporal.source, TimestampSource::FileSystemModified);
        assert!(!doc.extraction_status.warnings.is_empty());
    } else {
        let item = DiscoveredMediaItem {
            path: corrupt_path.clone(),
            file_size_bytes: 20,
            modified_at: None,
            category: MediaCategory::Photo,
            extension: "jpg".to_string(),
        };
        let doc = extractor
            .extract_file(&item, Some(Path::new("tests/fixtures")))
            .expect("Corrupt EXIF should not crash extractor");
        assert_eq!(doc.extraction_status.status, ExtractionResultKind::Partial);
        assert_eq!(doc.temporal.source, TimestampSource::FileSystemModified);
        assert!(!doc.extraction_status.warnings.is_empty());
    }
}

#[test]
fn test_extract_plain_image_filesystem_fallback() {
    let temp_file = NamedTempFile::new().expect("Failed to create temp file");
    let temp_path = temp_file.path().to_path_buf();

    let extractor = ExifMetadataExtractor::new();
    let item = DiscoveredMediaItem {
        path: temp_path.clone(),
        file_size_bytes: 0,
        modified_at: None,
        category: MediaCategory::Photo,
        extension: "tmp".to_string(),
    };

    let doc = extractor
        .extract_file(&item, temp_path.parent())
        .expect("Extraction should succeed with filesystem fallback");

    assert_eq!(doc.temporal.source, TimestampSource::FileSystemModified);
    assert_eq!(doc.file_context.file_size_bytes, 0);
    assert_eq!(doc.file_context.media_category, MediaCategory::Photo);
    assert_eq!(doc.extraction_status.status, ExtractionResultKind::Partial);
}

#[test]
fn test_geolocation_validation() {
    // Valid coordinates
    let loc = GeoLocation::new(37.7749, -122.4194, Some(15.0));
    assert!(loc.is_some());
    let loc = loc.unwrap();
    assert_eq!(loc.latitude, 37.7749);
    assert_eq!(loc.longitude, -122.4194);
    assert_eq!(loc.altitude_meters, Some(15.0));

    // Out of bounds latitude
    assert!(GeoLocation::new(95.0, 10.0, None).is_none());
    assert!(GeoLocation::new(-91.0, 10.0, None).is_none());

    // Out of bounds longitude
    assert!(GeoLocation::new(10.0, 185.0, None).is_none());
    assert!(GeoLocation::new(10.0, -181.0, None).is_none());

    // Zero/uncalibrated coordinates
    assert!(GeoLocation::new(0.0, 0.0, None).is_none());
}

#[test]
fn test_extract_valid_exif_photo() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = PathBuf::from(manifest_dir).join("tests/fixtures/valid_samples/valid_exif.jpg");
    assert!(path.exists(), "valid_exif.jpg fixture must exist");

    let extractor = ExifMetadataExtractor::new();
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let item = DiscoveredMediaItem {
        path: path.clone(),
        file_size_bytes: size,
        modified_at: None,
        category: MediaCategory::Photo,
        extension: "jpg".to_string(),
    };

    let doc = extractor
        .extract_file(&item, Some(Path::new(manifest_dir)))
        .expect("Extraction from valid EXIF photo must succeed");

    assert_eq!(doc.file_context.extension, "jpg");
    assert_eq!(doc.file_context.media_category, MediaCategory::Photo);
    assert_eq!(doc.extraction_status.status, ExtractionResultKind::Success);
    assert_eq!(doc.temporal.source, TimestampSource::ExifDateTimeOriginal);
    assert_eq!(doc.temporal.year, 2016);
    assert_eq!(doc.temporal.month, 5);
    assert_eq!(doc.temporal.day, 4);

    let dims = doc.dimensions.expect("Dimensions must be extracted");
    assert_eq!(dims.width, 15);
    assert_eq!(dims.height, 7);
}

#[test]
fn test_extract_valid_raw_tiff() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let path = PathBuf::from(manifest_dir).join("tests/fixtures/valid_samples/valid_raw.tif");
    assert!(path.exists(), "valid_raw.tif fixture must exist");

    let extractor = ExifMetadataExtractor::new();
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let item = DiscoveredMediaItem {
        path: path.clone(),
        file_size_bytes: size,
        modified_at: None,
        category: MediaCategory::RawPhoto,
        extension: "tif".to_string(),
    };

    let doc = extractor
        .extract_file(&item, Some(Path::new(manifest_dir)))
        .expect("Extraction from valid RAW TIFF must succeed");

    assert_eq!(doc.file_context.extension, "tif");
    assert_eq!(doc.file_context.media_category, MediaCategory::RawPhoto);
    let dims = doc
        .dimensions
        .expect("TIFF dimensions must be extracted via kamadak fallback");
    assert_eq!(dims.width, 15);
    assert_eq!(dims.height, 7);
    assert_eq!(dims.orientation, Some(1));
}
