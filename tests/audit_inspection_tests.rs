use metadata_extractor::domain::audit_models::{InspectionCategory, InspectionSample};
use std::path::PathBuf;
use std::str::FromStr;
use uuid::Uuid;

#[test]
fn test_inspection_category_parsing() {
    assert_eq!(
        InspectionCategory::from_str("problems").unwrap(),
        InspectionCategory::Problems
    );
    assert_eq!(
        InspectionCategory::from_str("failed").unwrap(),
        InspectionCategory::Problems
    );
    assert_eq!(
        InspectionCategory::from_str("missing-exif").unwrap(),
        InspectionCategory::MissingExif
    );
    assert_eq!(
        InspectionCategory::from_str("no-exif").unwrap(),
        InspectionCategory::MissingExif
    );
    assert_eq!(
        InspectionCategory::from_str("duplicates").unwrap(),
        InspectionCategory::Duplicates
    );
    assert_eq!(
        InspectionCategory::from_str("unreplicated").unwrap(),
        InspectionCategory::YetToReplicate
    );
    assert_eq!(
        InspectionCategory::from_str("marked-for-delete").unwrap(),
        InspectionCategory::MarkedForDelete
    );
    assert_eq!(
        InspectionCategory::from_str("unmastered").unwrap(),
        InspectionCategory::Unmastered
    );
    assert_eq!(
        InspectionCategory::from_str("replicas").unwrap(),
        InspectionCategory::Replicas
    );
    assert_eq!(
        InspectionCategory::from_str("replicated").unwrap(),
        InspectionCategory::Replicas
    );
    assert!(InspectionCategory::from_str("nonexistent_category").is_err());
}

#[test]
fn test_inspection_sample_serialization() {
    let sample = InspectionSample {
        id: Uuid::new_v4(),
        file_path: PathBuf::from("/Volumes/Media/Photos/sample.jpg"),
        file_size_bytes: 1048576,
        extension: "jpg".to_string(),
        category: InspectionCategory::Problems,
        diagnostic_detail: Some("Corrupt JPEG header".to_string()),
    };

    let json_str = serde_json::to_string(&sample).expect("Serialization failed");
    let deserialized: InspectionSample =
        serde_json::from_str(&json_str).expect("Deserialization failed");

    assert_eq!(deserialized.file_path, sample.file_path);
    assert_eq!(deserialized.file_size_bytes, 1048576);
    assert_eq!(deserialized.category, InspectionCategory::Problems);
    assert_eq!(
        deserialized.diagnostic_detail,
        Some("Corrupt JPEG header".to_string())
    );
}
