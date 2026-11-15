use metadata_extractor::domain::models::{DiscoveredMediaItem, MediaCategory};
use metadata_extractor::domain::ports::{MediaClassifier, MediaMetadataExtractor};
use metadata_extractor::infrastructure::classifier::PhfMediaClassifier;
use metadata_extractor::infrastructure::exif_parser::ExifMetadataExtractor;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

#[test]
fn test_nef_classifier_classification() {
    let classifier = PhfMediaClassifier::new();
    let nef_path = Path::new("/test/path/NIKON_001.NEF");
    let category = classifier.classify(nef_path);
    assert_eq!(category, Some(MediaCategory::RawPhoto));

    let nef_lower = Path::new("/test/path/dsc_0001.nef");
    assert_eq!(classifier.classify(nef_lower), Some(MediaCategory::RawPhoto));
}

#[test]
fn test_synthetic_tiff_subifd_dimension_extraction() {
    // Construct a synthetic minimal TIFF LE file with IFD0 and SubIFD1
    let mut file = NamedTempFile::new().expect("Failed creating temp file");
    let mut buf = Vec::new();

    // 1. TIFF Header: 'II' (0x4949), 42, IFD0 offset = 8
    buf.extend_from_slice(b"II");
    buf.extend_from_slice(&42u16.to_le_bytes());
    buf.extend_from_slice(&8u32.to_le_bytes());

    // 2. IFD0 at offset 8
    // Entries: 3
    // Entry 0: 0x0100 (ImageWidth), Short (3), count 1, val 160
    // Entry 1: 0x0101 (ImageLength), Short (3), count 1, val 120
    // Entry 2: 0x014A (SubIFDs), Long (4), count 1, offset = 50
    buf.extend_from_slice(&3u16.to_le_bytes()); // num_entries = 3

    // ImageWidth (160)
    buf.extend_from_slice(&0x0100u16.to_le_bytes());
    buf.extend_from_slice(&3u16.to_le_bytes()); // Short
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&160u16.to_le_bytes());
    buf.extend_from_slice(&[0u8; 2]); // pad to 4 bytes

    // ImageLength (120)
    buf.extend_from_slice(&0x0101u16.to_le_bytes());
    buf.extend_from_slice(&3u16.to_le_bytes()); // Short
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&120u16.to_le_bytes());
    buf.extend_from_slice(&[0u8; 2]);

    // SubIFD offset (50)
    buf.extend_from_slice(&0x014Au16.to_le_bytes());
    buf.extend_from_slice(&4u16.to_le_bytes()); // Long
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&50u32.to_le_bytes());

    // Next IFD offset = 0
    buf.extend_from_slice(&0u32.to_le_bytes());

    // Pad buffer up to offset 50
    while buf.len() < 50 {
        buf.push(0);
    }

    // 3. SubIFD at offset 50
    // Entries: 2
    // Entry 0: 0x0100 (ImageWidth), Long (4), count 1, val 6036
    // Entry 1: 0x0101 (ImageLength), Long (4), count 1, val 4020
    buf.extend_from_slice(&2u16.to_le_bytes());

    // ImageWidth (6036)
    buf.extend_from_slice(&0x0100u16.to_le_bytes());
    buf.extend_from_slice(&4u16.to_le_bytes()); // Long
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&6036u32.to_le_bytes());

    // ImageLength (4020)
    buf.extend_from_slice(&0x0101u16.to_le_bytes());
    buf.extend_from_slice(&4u16.to_le_bytes()); // Long
    buf.extend_from_slice(&1u32.to_le_bytes());
    buf.extend_from_slice(&4020u32.to_le_bytes());

    file.write_all(&buf).expect("Failed writing synthetic TIFF");
    file.flush().expect("Failed flushing");

    let dims = ExifMetadataExtractor::try_parse_tiff_subifd_dimensions(file.path());
    assert_eq!(dims, Some((6036, 4020)));
}

#[test]
fn test_real_nikon_nef_extraction_if_available() {
    let sample_path = PathBuf::from("/Volumes/Seagate Backup Plus Drive/SPSA/Halloween/SPS_1127.NEF");
    if !sample_path.exists() {
        eprintln!("Skipping real NEF test: sample path not accessible");
        return;
    }

    let extractor = ExifMetadataExtractor::new();
    let fs_meta = std::fs::metadata(&sample_path).expect("Failed reading metadata");
    let item = DiscoveredMediaItem::new(
        sample_path.clone(),
        fs_meta.len(),
        fs_meta.modified().ok(),
        MediaCategory::RawPhoto,
        "nef".to_string(),
    );

    let doc = extractor
        .extract_file(&item, Some(sample_path.parent().unwrap()))
        .expect("Extraction failed");

    assert_eq!(doc.file_context.media_category, MediaCategory::RawPhoto);
    assert!(doc.dimensions.is_some());
    let dims = doc.dimensions.unwrap();
    assert_eq!(dims.width, 6036);
    assert_eq!(dims.height, 4020);
    assert!(doc.camera.is_some());
    let cam = doc.camera.unwrap();
    assert_eq!(cam.make.as_deref(), Some("NIKON CORPORATION"));
}
