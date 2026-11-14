use chrono::Utc;
use metadata_extractor::application::AuditService;
use metadata_extractor::domain::audit_models::CatalogAuditReport;

fn create_test_report(
    unmastered: u64,
    yet_to_replicate: u64,
    marked_for_delete: u64,
    problem_files: u64,
    missing_exif: u64,
) -> CatalogAuditReport {
    CatalogAuditReport {
        total_files: 500,
        extension_counts: Vec::new(),
        copied_to_master: 400,
        total_replicas: 0,
        replica_drives: Vec::new(),
        yet_to_replicate,
        duplicates_detected: marked_for_delete,
        marked_for_delete,
        missing_exif,
        problem_files,
        unmastered_files: unmastered,
        generated_at: Utc::now(),
        recommended_action: None,
        samples: None,
    }
}

#[test]
fn test_recommendation_priority_unmastered() {
    let report = create_test_report(10, 5, 2, 1, 3);
    let rec = AuditService::evaluate_recommendation(&report);
    assert_eq!(rec.priority, 1);
    assert_eq!(rec.command, "cargo run -- ingest");
}

#[test]
fn test_recommendation_priority_replicate() {
    let report = create_test_report(0, 5, 2, 1, 3);
    let rec = AuditService::evaluate_recommendation(&report);
    assert_eq!(rec.priority, 2);
    assert_eq!(rec.command, "cargo run -- sync-replicas");
}

#[test]
fn test_recommendation_priority_prune() {
    let report = create_test_report(0, 0, 7, 1, 3);
    let rec = AuditService::evaluate_recommendation(&report);
    assert_eq!(rec.priority, 3);
    assert_eq!(rec.command, "cargo run -- dedup prune --execute");
}

#[test]
fn test_recommendation_priority_problems() {
    let report = create_test_report(0, 0, 0, 4, 3);
    let rec = AuditService::evaluate_recommendation(&report);
    assert_eq!(rec.priority, 4);
    assert_eq!(rec.command, "cargo run -- report inspect problems");
}

#[test]
fn test_recommendation_priority_missing_exif() {
    let report = create_test_report(0, 0, 0, 0, 8);
    let rec = AuditService::evaluate_recommendation(&report);
    assert_eq!(rec.priority, 5);
    assert_eq!(rec.command, "cargo run -- report inspect missing-exif");
}

#[test]
fn test_recommendation_all_clean() {
    let report = create_test_report(0, 0, 0, 0, 0);
    let rec = AuditService::evaluate_recommendation(&report);
    assert_eq!(rec.priority, 6);
    assert_eq!(rec.title, "Catalog Fully Synchronized & Healthy");
}
