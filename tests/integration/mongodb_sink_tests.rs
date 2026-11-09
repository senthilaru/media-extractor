use metadata_extractor::domain::metadata_models::DatabaseSinkConfig;
use metadata_extractor::domain::ports::{
    BatchMetadataExtractor, DirectoryScanner, MetadataStorageSink,
};
use metadata_extractor::infrastructure::mongodb_sink::MongoDbStorageSink;
use metadata_extractor::MetadataExtractionEngine;
use std::path::PathBuf;

fn get_test_mongodb_uri() -> String {
    let _ = dotenvy::dotenv();
    if let Ok(uri) = std::env::var("MONGODB_URI") {
        let pass = std::env::var("MONGODB_PASSWORD").ok();
        return DatabaseSinkConfig::interpolate_secret(&uri, pass.as_deref());
    }
    let pass =
        std::env::var("MONGODB_PASSWORD").unwrap_or_else(|_| "test_pass_placeholder".to_string());
    format!(
        "mongodb://media_app:{}@localhost:27017/media_index_db?authSource=media_index_db",
        pass
    )
}

#[tokio::test]
async fn test_mongodb_fail_fast_bad_connection() {
    // Unreachable host/port with short timeout
    let uri = "mongodb://bad_user:bad_pass@127.0.0.1:27019/non_existent_db";
    let mut config = DatabaseSinkConfig::new(uri, None);
    config.timeout_seconds = 1;

    let result = MongoDbStorageSink::connect(config).await;
    assert!(result.is_err(), "Must fail fast when host is unreachable");
}

#[tokio::test]
async fn test_mongodb_live_ingestion_and_idempotency() {
    let uri = get_test_mongodb_uri();
    let config = DatabaseSinkConfig::new(&uri, Some("test_media_assets".to_string()));

    let sink = match MongoDbStorageSink::connect(config).await {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Skipping live MongoDB test (could not connect): {}", e);
            return;
        }
    };

    // Ensure indexes
    sink.ensure_indexes()
        .await
        .expect("ensure_indexes must succeed");

    // Extract valid samples
    let fixture_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/valid_samples");
    let engine = MetadataExtractionEngine::new();
    let scan_engine = metadata_extractor::application::engine::ScanEngine::new();
    let scan_config = metadata_extractor::domain::ports::ScanConfig {
        root_paths: vec![fixture_dir.clone()],
        ..Default::default()
    };

    let (_session, mut stream) = scan_engine.start_scan(scan_config).await.unwrap();
    let mut all_docs = Vec::new();
    while let Some(batch) = stream.batches.recv().await {
        let docs = engine.extract_batch(batch, Some(&fixture_dir)).await;
        all_docs.extend(docs);
    }

    assert!(!all_docs.is_empty(), "Must have extracted docs");

    // First save: should upsert documents
    let stats1 = sink
        .save_batch(&all_docs)
        .await
        .expect("save_batch must succeed");
    assert!(stats1.upserted_count > 0 || stats1.matched_count > 0);

    // Second save (re-scan): should match/update existing documents, upserted_count should be 0
    let stats2 = sink
        .save_batch(&all_docs)
        .await
        .expect("save_batch second run must succeed");
    assert_eq!(
        stats2.upserted_count, 0,
        "Re-scan must not upsert new documents"
    );
    assert_eq!(
        stats2.matched_count,
        all_docs.len() as u64,
        "All documents must match on re-scan"
    );
}

#[test]
fn test_cli_mongodb_fail_fast_bad_credentials() {
    let fixture_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/valid_samples");
    let bin_path = env!("CARGO_BIN_EXE_metadata-extractor");

    let output = std::process::Command::new(bin_path)
        .arg(&fixture_dir)
        .arg("--mongodb-uri")
        .arg("mongodb://media_app:wrong_password@localhost:27017/media_index_db?authSource=media_index_db")
        .output()
        .expect("Failed to execute binary");

    assert!(
        !output.status.success(),
        "Bad credentials must return error exit code"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("media_app:***@localhost:27017"),
        "Password must be sanitized in logs/errors"
    );
    assert!(stderr.contains("Authentication failed") || stderr.contains("AuthenticationFailed"));
}

#[test]
fn test_cli_mongodb_live_direct_scan() {
    let uri = get_test_mongodb_uri();
    // Pre-check if MongoDB is reachable with the credentials
    let rt = tokio::runtime::Runtime::new().unwrap();
    let is_connected = rt.block_on(async {
        let config = DatabaseSinkConfig::new(&uri, None);
        MongoDbStorageSink::connect(config).await.is_ok()
    });

    if !is_connected {
        eprintln!("Skipping live CLI MongoDB test (database not reachable or not authenticated)");
        return;
    }

    let fixture_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/valid_samples");
    let bin_path = env!("CARGO_BIN_EXE_metadata-extractor");

    let output = std::process::Command::new(bin_path)
        .arg(&fixture_dir)
        .arg("--mongodb-uri")
        .arg(&uri)
        .arg("--collection")
        .arg("cli_live_test_assets")
        .output()
        .expect("Failed to execute binary");

    assert!(
        output.status.success(),
        "Live scan with valid credentials must succeed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Scan & Ingest completed"),
        "Must complete scan & ingest"
    );
    assert!(
        stderr.contains("media_app:***@localhost:27017"),
        "Credentials must be masked on stderr"
    );
}
