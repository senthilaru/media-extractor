use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use uuid::Uuid;

use metadata_extractor::application::engine::ScanEngine;
use metadata_extractor::application::metadata_engine::MetadataExtractionEngine;
use metadata_extractor::application::pipeline::ExtractionPipeline;
use metadata_extractor::domain::metadata_models::DatabaseSinkConfig;
use metadata_extractor::domain::ports::{MetadataStorageSink, ProgressSink};
use metadata_extractor::domain::progress_models::{
    ExtractionMode, ExtractionPerformanceProfile, ScanProgressSnapshot,
};
use metadata_extractor::infrastructure::mongodb_sink::MongoDbStorageSink;

/// Minimal silent progress sink for testing pipeline executions.
struct TestProgressSink;

impl ProgressSink for TestProgressSink {
    fn render_progress(&self, _snapshot: &ScanProgressSnapshot) {}
    fn render_summary(
        &self,
        _summary: &metadata_extractor::domain::progress_models::ExecutionSummary,
    ) {
    }
}

fn create_sample_jpeg(path: &PathBuf, extra_bytes: &[u8]) {
    let mut file = File::create(path).expect("Failed to create test image");
    // JPEG header: SOI, APP0, EOI
    file.write_all(&[
        0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0x01, 0x01, 0x00, 0x00,
        0x01, 0x00, 0x01, 0x00, 0x00,
    ])
    .expect("Failed to write SOI/APP0");
    if !extra_bytes.is_empty() {
        file.write_all(extra_bytes)
            .expect("Failed to write extra payload");
    }
    file.write_all(&[0xFF, 0xD9]).expect("Failed to write EOI");
    file.flush().expect("Failed to flush");
}

#[tokio::test]
async fn test_incremental_scan_pipeline_e2e() {
    dotenvy::dotenv().ok();
    let uri = match std::env::var("MONGODB_URI") {
        Ok(u) => {
            let pass = std::env::var("MONGODB_PASSWORD").ok();
            DatabaseSinkConfig::interpolate_secret(&u, pass.as_deref())
        }
        Err(_) => {
            eprintln!("Skipping live MongoDB integration test: MONGODB_URI not set");
            return;
        }
    };

    let col_name = format!("test_incr_pipe_{}", Uuid::new_v4().simple());
    let config = DatabaseSinkConfig::new(uri, Some(col_name));

    let storage_sink = match MongoDbStorageSink::connect(config).await {
        Ok(s) => Arc::new(s),
        Err(_) => {
            eprintln!("Skipping live MongoDB test: connection failed");
            return;
        }
    };

    let _ = storage_sink.ensure_indexes().await;

    // Create temp directory with 3 media files
    let dir = tempdir().expect("Failed to create tempdir");
    let root = dir.path().to_path_buf();

    let img1 = root.join("img1.jpg");
    let img2 = root.join("img2.jpg");
    let img3 = root.join("img3.jpg");

    create_sample_jpeg(&img1, b"first");
    create_sample_jpeg(&img2, b"second");
    create_sample_jpeg(&img3, b"third");

    let scan_engine = Arc::new(ScanEngine::new());
    let default_profile = ExtractionPerformanceProfile {
        mode: ExtractionMode::Fast,
        max_hash_size_bytes: None,
        enable_perceptual: false,
        enable_cryptographic_hash: false,
        worker_concurrency: 2,
        batch_size: 10,
        channel_capacity: 10,
        update_interval_ms: 100,
        force_reindex: false,
    };

    let meta_engine = Arc::new(MetadataExtractionEngine::with_performance_profile(
        default_profile.clone(),
    ));
    let progress_sink: Arc<dyn ProgressSink> = Arc::new(TestProgressSink);

    // --- RUN 1: Initial Scan (cold cache) ---
    let pipeline1 = ExtractionPipeline::new(
        Arc::clone(&scan_engine),
        Arc::clone(&meta_engine),
        default_profile.clone(),
        Some(Arc::clone(&storage_sink) as Arc<dyn MetadataStorageSink>),
        Arc::clone(&progress_sink),
    );

    let cancel1 = Arc::new(AtomicBool::new(false));
    let summary1 = pipeline1
        .execute(root.clone(), None, cancel1, false)
        .await
        .expect("Pipeline run 1 failed");

    assert_eq!(summary1.total_discovered, 3);
    assert_eq!(
        summary1.total_processed, 3,
        "Run 1 must extract all 3 files"
    );
    assert_eq!(summary1.total_skipped, 0, "Run 1 should skip 0 files");

    // Allow MongoDB bulk writes to settle
    tokio::time::sleep(Duration::from_millis(100)).await;

    // --- RUN 2: Re-Scan Unmodified Directory (warm cache) ---
    let pipeline2 = ExtractionPipeline::new(
        Arc::clone(&scan_engine),
        Arc::clone(&meta_engine),
        default_profile.clone(),
        Some(Arc::clone(&storage_sink) as Arc<dyn MetadataStorageSink>),
        Arc::clone(&progress_sink),
    );

    let cancel2 = Arc::new(AtomicBool::new(false));
    let summary2 = pipeline2
        .execute(root.clone(), None, cancel2, false)
        .await
        .expect("Pipeline run 2 failed");

    assert_eq!(summary2.total_discovered, 3);
    assert_eq!(
        summary2.total_skipped, 3,
        "Run 2 must skip all 3 unchanged files"
    );
    assert_eq!(summary2.total_processed, 0, "Run 2 must extract 0 files");

    // --- RUN 3: Modify 1 file and add 1 new file ---
    // Modify img2 (change content and update mtime)
    create_sample_jpeg(&img2, b"second_modified_larger_content");
    // Add new img4
    let img4 = root.join("img4.jpg");
    create_sample_jpeg(&img4, b"fourth_brand_new");

    let pipeline3 = ExtractionPipeline::new(
        Arc::clone(&scan_engine),
        Arc::clone(&meta_engine),
        default_profile.clone(),
        Some(Arc::clone(&storage_sink) as Arc<dyn MetadataStorageSink>),
        Arc::clone(&progress_sink),
    );

    let cancel3 = Arc::new(AtomicBool::new(false));
    let summary3 = pipeline3
        .execute(root.clone(), None, cancel3, false)
        .await
        .expect("Pipeline run 3 failed");

    assert_eq!(summary3.total_discovered, 4);
    assert_eq!(summary3.total_skipped, 2, "img1 and img3 must be skipped");
    assert_eq!(
        summary3.total_processed, 2,
        "img2 (modified) and img4 (new) must be extracted"
    );

    // --- RUN 4: Force Re-index on all 4 files ---
    let mut force_profile = default_profile.clone();
    force_profile.force_reindex = true;

    let pipeline4 = ExtractionPipeline::new(
        Arc::clone(&scan_engine),
        Arc::clone(&meta_engine),
        force_profile,
        Some(Arc::clone(&storage_sink) as Arc<dyn MetadataStorageSink>),
        Arc::clone(&progress_sink),
    );

    let cancel4 = Arc::new(AtomicBool::new(false));
    let summary4 = pipeline4
        .execute(root.clone(), None, cancel4, false)
        .await
        .expect("Pipeline run 4 failed");

    assert_eq!(summary4.total_discovered, 4);
    assert_eq!(
        summary4.total_skipped, 0,
        "Force re-index must skip 0 files"
    );
    assert_eq!(
        summary4.total_processed, 4,
        "Force re-index must re-extract all 4 files"
    );
    assert!(summary4.force_reindex);
}
