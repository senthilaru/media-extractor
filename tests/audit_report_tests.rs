use chrono::Utc;
use metadata_extractor::domain::audit_models::{
    CatalogAuditReport, CommandRecommendation, ExtensionCount, ReplicaDriveCount,
};

#[test]
fn test_catalog_audit_report_serialization() {
    let report = CatalogAuditReport {
        total_files: 100,
        extension_counts: vec![
            ExtensionCount {
                extension: "jpg".to_string(),
                count: 60,
                percentage: 60.0,
            },
            ExtensionCount {
                extension: "mp4".to_string(),
                count: 40,
                percentage: 40.0,
            },
        ],
        copied_to_master: 80,
        total_replicas: 75,
        replica_drives: vec![ReplicaDriveCount {
            drive_id: "replica_1".to_string(),
            replicated_count: 75,
            pending_count: 5,
        }],
        yet_to_replicate: 5,
        duplicates_detected: 15,
        marked_for_delete: 5,
        missing_exif: 8,
        problem_files: 2,
        unmastered_files: 20,
        generated_at: Utc::now(),
        recommended_action: Some(CommandRecommendation {
            priority: 1,
            title: "Ingest Unmastered Media Assets".to_string(),
            reason: "20 scanned media assets are waiting to be ingested".to_string(),
            command: "cargo run -- ingest".to_string(),
        }),
        samples: None,
    };

    let json_str = serde_json::to_string(&report).expect("Serialization failed");
    let deserialized: CatalogAuditReport =
        serde_json::from_str(&json_str).expect("Deserialization failed");

    assert_eq!(deserialized.total_files, 100);
    assert_eq!(deserialized.extension_counts.len(), 2);
    assert_eq!(deserialized.extension_counts[0].extension, "jpg");
    assert_eq!(deserialized.copied_to_master, 80);
    assert_eq!(deserialized.total_replicas, 75);
    assert_eq!(deserialized.replica_drives.len(), 1);
    assert_eq!(deserialized.replica_drives[0].drive_id, "replica_1");
    assert_eq!(deserialized.replica_drives[0].replicated_count, 75);
    assert_eq!(deserialized.replica_drives[0].pending_count, 5);
    assert_eq!(deserialized.yet_to_replicate, 5);
    assert_eq!(deserialized.duplicates_detected, 15);
    assert_eq!(deserialized.marked_for_delete, 5);
    assert_eq!(deserialized.missing_exif, 8);
    assert_eq!(deserialized.problem_files, 2);
    assert_eq!(deserialized.unmastered_files, 20);
    assert_eq!(
        deserialized.recommended_action.unwrap().command,
        "cargo run -- ingest"
    );
}

#[test]
fn test_empty_catalog_audit_report() {
    let report = CatalogAuditReport {
        total_files: 0,
        extension_counts: Vec::new(),
        copied_to_master: 0,
        total_replicas: 0,
        replica_drives: Vec::new(),
        yet_to_replicate: 0,
        duplicates_detected: 0,
        marked_for_delete: 0,
        missing_exif: 0,
        problem_files: 0,
        unmastered_files: 0,
        generated_at: Utc::now(),
        recommended_action: None,
        samples: None,
    };

    let json_str = serde_json::to_string(&report).expect("Serialization failed");
    let deserialized: CatalogAuditReport =
        serde_json::from_str(&json_str).expect("Deserialization failed");

    assert_eq!(deserialized.total_files, 0);
    assert!(deserialized.extension_counts.is_empty());
}
