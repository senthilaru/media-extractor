use chrono::{DateTime, Utc};
use metadata_extractor::domain::metadata_models::{
    DatabaseSinkConfig, ExtractionStatus, FileContext, FilterEvaluationResult,
    MediaMetadataDocument, TemporalMetadata, TimestampSource,
};
use metadata_extractor::domain::models::{DiscoveredMediaItem, MediaCategory};
use metadata_extractor::domain::ports::MetadataStorageSink;
use metadata_extractor::infrastructure::mongodb_sink::MongoDbStorageSink;
use std::path::PathBuf;
use std::time::SystemTime;
use uuid::Uuid;

#[test]
fn test_filter_evaluation_result_default() {
    let result = FilterEvaluationResult::default();
    assert_eq!(result.unmodified_items.len(), 0);
    assert_eq!(result.items_to_extract.len(), 0);
    assert_eq!(result.skipped_bytes, 0);
    assert_eq!(result.extract_bytes, 0);
}

#[tokio::test]
async fn test_mongodb_filter_unmodified_items_empty() {
    dotenvy::dotenv().ok();
    let uri =
        std::env::var("MONGODB_URI").unwrap_or_else(|_| "mongodb://127.0.0.1:27017".to_string());
    let config =
        DatabaseSinkConfig::new(uri, Some(format!("test_filter_empty_{}", Uuid::new_v4())));

    if let Ok(sink) = MongoDbStorageSink::connect(config).await {
        let result = sink.filter_unmodified_items(&[]).await;
        assert!(result.is_ok());
        let eval = result.unwrap();
        assert_eq!(eval.unmodified_items.len(), 0);
        assert_eq!(eval.items_to_extract.len(), 0);
        assert_eq!(eval.skipped_bytes, 0);
        assert_eq!(eval.extract_bytes, 0);
    }
}

#[tokio::test]
async fn test_mongodb_filter_unmodified_items_live() {
    dotenvy::dotenv().ok();
    let uri = match std::env::var("MONGODB_URI") {
        Ok(u) => {
            let pass = std::env::var("MONGODB_PASSWORD").ok();
            DatabaseSinkConfig::interpolate_secret(&u, pass.as_deref())
        }
        Err(_) => {
            eprintln!("Skipping live MongoDB test: MONGODB_URI not set");
            return;
        }
    };
    let col_name = format!("test_filter_live_{}", Uuid::new_v4().simple());
    let config = DatabaseSinkConfig::new(uri, Some(col_name));

    let sink = match MongoDbStorageSink::connect(config).await {
        Ok(s) => s,
        Err(_) => {
            eprintln!("Skipping live MongoDB test: connection unavailable");
            return;
        }
    };

    let _ = sink.ensure_indexes().await;

    let now_st = SystemTime::now();
    let now_dt: DateTime<Utc> = now_st.into();

    let path_unmodified = PathBuf::from("/media/photo_unmodified.jpg");
    let path_size_changed = PathBuf::from("/media/photo_size_changed.jpg");
    let path_mtime_changed = PathBuf::from("/media/photo_mtime_changed.jpg");
    let path_new_file = PathBuf::from("/media/photo_new.jpg");

    // Pre-populate DB with existing records
    let existing_docs = vec![
        MediaMetadataDocument {
            id: Uuid::new_v4(),
            file_context: FileContext {
                scanned_root: PathBuf::from("/media"),
                file_path: path_unmodified.clone(),
                relative_path: PathBuf::from("photo_unmodified.jpg"),
                file_name: "photo_unmodified.jpg".to_string(),
                file_size_bytes: 10_000,
                extension: "jpg".to_string(),
                media_category: MediaCategory::Photo,
                modified_at: Some(now_dt),
            },
            temporal: TemporalMetadata::from_utc_datetime(
                now_dt,
                TimestampSource::ExifDateTimeOriginal,
            ),
            camera: None,
            exposure: None,
            location: None,
            dimensions: None,
            video: None,
            fingerprint: None,
            extraction_status: ExtractionStatus::default(),
        },
        MediaMetadataDocument {
            id: Uuid::new_v4(),
            file_context: FileContext {
                scanned_root: PathBuf::from("/media"),
                file_path: path_size_changed.clone(),
                relative_path: PathBuf::from("photo_size_changed.jpg"),
                file_name: "photo_size_changed.jpg".to_string(),
                file_size_bytes: 5_000, // stored as 5000 bytes
                extension: "jpg".to_string(),
                media_category: MediaCategory::Photo,
                modified_at: Some(now_dt),
            },
            temporal: TemporalMetadata::from_utc_datetime(
                now_dt,
                TimestampSource::ExifDateTimeOriginal,
            ),
            camera: None,
            exposure: None,
            location: None,
            dimensions: None,
            video: None,
            fingerprint: None,
            extraction_status: ExtractionStatus::default(),
        },
        MediaMetadataDocument {
            id: Uuid::new_v4(),
            file_context: FileContext {
                scanned_root: PathBuf::from("/media"),
                file_path: path_mtime_changed.clone(),
                relative_path: PathBuf::from("photo_mtime_changed.jpg"),
                file_name: "photo_mtime_changed.jpg".to_string(),
                file_size_bytes: 20_000,
                extension: "jpg".to_string(),
                media_category: MediaCategory::Photo,
                modified_at: Some(now_dt - chrono::Duration::hours(24)), // stored with old mtime
            },
            temporal: TemporalMetadata::from_utc_datetime(
                now_dt,
                TimestampSource::ExifDateTimeOriginal,
            ),
            camera: None,
            exposure: None,
            location: None,
            dimensions: None,
            video: None,
            fingerprint: None,
            extraction_status: ExtractionStatus::default(),
        },
    ];

    sink.save_batch(&existing_docs)
        .await
        .expect("Failed to save initial documents");

    // Discovered items in next scan run:
    let discovered_items = vec![
        // 1. Unmodified: exact same path, size (10000), and mtime
        DiscoveredMediaItem {
            path: path_unmodified.clone(),
            file_size_bytes: 10_000,
            modified_at: Some(now_st),
            category: MediaCategory::Photo,
            extension: "jpg".to_string(),
        },
        // 2. Modified size: on disk size is now 8000, DB has 5000
        DiscoveredMediaItem {
            path: path_size_changed.clone(),
            file_size_bytes: 8_000,
            modified_at: Some(now_st),
            category: MediaCategory::Photo,
            extension: "jpg".to_string(),
        },
        // 3. Modified mtime: on disk mtime is now_st, DB has now_st - 24 hours
        DiscoveredMediaItem {
            path: path_mtime_changed.clone(),
            file_size_bytes: 20_000,
            modified_at: Some(now_st),
            category: MediaCategory::Photo,
            extension: "jpg".to_string(),
        },
        // 4. Brand new file: not in DB
        DiscoveredMediaItem {
            path: path_new_file.clone(),
            file_size_bytes: 15_000,
            modified_at: Some(now_st),
            category: MediaCategory::Photo,
            extension: "jpg".to_string(),
        },
    ];

    let result = sink
        .filter_unmodified_items(&discovered_items)
        .await
        .expect("filter_unmodified_items failed");

    assert_eq!(
        result.unmodified_items.len(),
        1,
        "Only 1 item should be recognized as unmodified"
    );
    assert_eq!(result.unmodified_items[0].path, path_unmodified);
    assert_eq!(result.skipped_bytes, 10_000);

    assert_eq!(
        result.items_to_extract.len(),
        3,
        "3 items should require extraction"
    );
    let extract_paths: Vec<PathBuf> = result
        .items_to_extract
        .iter()
        .map(|it| it.path.clone())
        .collect();
    assert!(extract_paths.contains(&path_size_changed));
    assert!(extract_paths.contains(&path_mtime_changed));
    assert!(extract_paths.contains(&path_new_file));
    assert_eq!(result.extract_bytes, 8_000 + 20_000 + 15_000);
}
