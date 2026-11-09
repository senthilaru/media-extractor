use std::path::PathBuf;
use uuid::Uuid;

use metadata_extractor::application::ProgressTracker;
use metadata_extractor::domain::ports::ProgressReporter;
use metadata_extractor::domain::progress_models::{ExtractionMode, OperationalPhase};

#[test]
fn test_progress_tracker_initialization() {
    let session_id = Uuid::new_v4();
    let tracker = ProgressTracker::new(
        session_id,
        PathBuf::from("/test/media"),
        ExtractionMode::Fast,
    );

    let snapshot = tracker.snapshot();
    assert_eq!(snapshot.session_id, session_id);
    assert_eq!(snapshot.active_phase, OperationalPhase::Discovery);
    assert_eq!(snapshot.items_discovered, 0);
    assert_eq!(snapshot.items_processed, 0);
    assert_eq!(snapshot.items_skipped, 0);
    assert_eq!(snapshot.bytes_skipped, 0);
    assert_eq!(snapshot.errors_encountered, 0);
    assert_eq!(snapshot.warnings_encountered, 0);
    assert!(!snapshot.is_discovery_complete);
}

#[test]
fn test_progress_tracker_record_progress_and_snapshot() {
    let session_id = Uuid::new_v4();
    let tracker = ProgressTracker::new(
        session_id,
        PathBuf::from("/test/media"),
        ExtractionMode::Balanced,
    );

    tracker.record_directory_scanned();
    tracker.record_discovered(500, 10_000_000);
    tracker.set_phase(OperationalPhase::Extraction);
    tracker.record_processed(200, 4_000_000);
    tracker.record_skipped(50, 1_000_000);
    tracker.record_error();
    tracker.record_warning();

    let snap = tracker.snapshot();
    assert_eq!(snap.active_phase, OperationalPhase::Extraction);
    assert_eq!(snap.directories_scanned, 1);
    assert_eq!(snap.items_discovered, 500);
    assert_eq!(snap.bytes_discovered, 10_000_000);
    assert_eq!(snap.items_processed, 200);
    assert_eq!(snap.bytes_processed, 4_000_000);
    assert_eq!(snap.items_skipped, 50);
    assert_eq!(snap.bytes_skipped, 1_000_000);
    assert_eq!(snap.errors_encountered, 1);
    assert_eq!(snap.warnings_encountered, 1);
}

#[test]
fn test_progress_tracker_finish_summary() {
    let session_id = Uuid::new_v4();
    let target = PathBuf::from("/spsa/temp");
    let tracker =
        ProgressTracker::with_options(session_id, target.clone(), ExtractionMode::Fast, true);

    tracker.record_discovered(100, 1_000_000);
    tracker.record_processed(80, 800_000);
    tracker.record_skipped(20, 200_000);
    tracker.record_db_stats(80, 0, 20, 0);

    let summary = tracker.finish(false);
    assert_eq!(summary.session_id, session_id);
    assert_eq!(summary.target_path, target);
    assert_eq!(summary.mode, ExtractionMode::Fast);
    assert_eq!(summary.total_discovered, 100);
    assert_eq!(summary.total_processed, 80);
    assert_eq!(summary.total_skipped, 20);
    assert_eq!(summary.total_bytes_processed, 800_000);
    assert_eq!(summary.total_bytes_skipped, 200_000);
    assert_eq!(summary.db_upserted, 80);
    assert_eq!(summary.extraction_errors, 0);
    assert!(!summary.was_cancelled);
    assert!(summary.force_reindex);
}

#[test]
fn test_progress_tracker_cancellation() {
    let session_id = Uuid::new_v4();
    let tracker = ProgressTracker::new(
        session_id,
        PathBuf::from("/spsa/temp"),
        ExtractionMode::Full,
    );

    tracker.record_discovered(500, 50_000_000);
    tracker.record_processed(120, 12_000_000);

    let summary = tracker.finish(true);
    assert!(summary.was_cancelled);
    assert_eq!(summary.total_discovered, 500);
    assert_eq!(summary.total_processed, 120);
}
