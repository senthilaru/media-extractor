use std::fs::File;
use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::Mutex;

use metadata_extractor::application::engine::ScanEngine;
use metadata_extractor::application::pipeline::ExtractionPipeline;
use metadata_extractor::application::MetadataExtractionEngine;
use metadata_extractor::domain::ports::ProgressSink;
use metadata_extractor::domain::progress_models::{
    ExecutionSummary, ExtractionMode, ExtractionPerformanceProfile, ScanProgressSnapshot,
};

struct MockProgressSink {
    snapshots_received: std::sync::atomic::AtomicU64,
    summary_received: std::sync::atomic::AtomicBool,
}

impl MockProgressSink {
    fn new() -> Self {
        Self {
            snapshots_received: std::sync::atomic::AtomicU64::new(0),
            summary_received: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

impl ProgressSink for MockProgressSink {
    fn render_progress(&self, _snapshot: &ScanProgressSnapshot) {
        self.snapshots_received
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn render_summary(&self, _summary: &ExecutionSummary) {
        self.summary_received
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

#[tokio::test]
async fn test_pipeline_fast_mode_execution() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    // Create a few dummy photo files with valid extensions
    for i in 0..5 {
        let file_path = root.join(format!("photo_{}.jpg", i));
        let mut f = File::create(&file_path).unwrap();
        // Write standard 1KB dummy content
        f.write_all(&vec![0xAA; 1024]).unwrap();
    }

    let scan_engine = Arc::new(ScanEngine::new());
    let mut profile = ExtractionPerformanceProfile::fast();
    profile.batch_size = 2;

    let metadata_engine = Arc::new(MetadataExtractionEngine::with_performance_profile(
        profile.clone(),
    ));
    let mock_sink = Arc::new(MockProgressSink::new());

    let pipeline = ExtractionPipeline::new(
        scan_engine,
        metadata_engine,
        profile,
        None,
        mock_sink.clone(),
    );

    let cancel_token = Arc::new(AtomicBool::new(false));
    let out_file_path = root.join("output.jsonl");
    let file = tokio::fs::File::create(&out_file_path).await.unwrap();
    let out_buf =
        Arc::new(Mutex::new(Box::new(tokio::io::BufWriter::new(file))
            as Box<dyn tokio::io::AsyncWrite + Unpin + Send>));

    let summary = pipeline
        .execute(
            root.to_path_buf(),
            Some(out_buf.clone()),
            cancel_token,
            false,
        )
        .await
        .unwrap();

    assert_eq!(summary.mode, ExtractionMode::Fast);
    assert_eq!(summary.total_discovered, 5);
    assert_eq!(summary.total_processed, 5);
    assert_eq!(summary.extraction_errors, 0);
    assert!(!summary.was_cancelled);
    assert!(mock_sink
        .summary_received
        .load(std::sync::atomic::Ordering::Relaxed));

    // Verify output stream contains valid JSON without progress bar pollution
    let text = tokio::fs::read_to_string(&out_file_path).await.unwrap();
    let lines: Vec<&str> = text.lines().filter(|l| !l.is_empty()).collect();
    assert_eq!(lines.len(), 5);

    for line in lines {
        let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
        // In fast mode, fingerprint is skipped (None)
        assert!(parsed.get("fingerprint").is_none());
    }
}

#[tokio::test]
async fn test_pipeline_cancellation() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    for i in 0..10 {
        let file_path = root.join(format!("image_{}.png", i));
        let mut f = File::create(&file_path).unwrap();
        f.write_all(&vec![0xBB; 2048]).unwrap();
    }

    let scan_engine = Arc::new(ScanEngine::new());
    let profile = ExtractionPerformanceProfile::default();
    let metadata_engine = Arc::new(MetadataExtractionEngine::with_performance_profile(
        profile.clone(),
    ));
    let mock_sink = Arc::new(MockProgressSink::new());

    let pipeline = ExtractionPipeline::new(
        scan_engine,
        metadata_engine,
        profile,
        None,
        mock_sink.clone(),
    );

    let cancel_token = Arc::new(AtomicBool::new(true)); // Pre-cancelled
    let summary = pipeline
        .execute(root.to_path_buf(), None, cancel_token, false)
        .await
        .unwrap();

    assert!(summary.was_cancelled);
}
