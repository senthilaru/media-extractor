use metadata_extractor::domain::metadata_models::DatabaseSinkConfig;

#[test]
fn test_database_sink_config_defaults() {
    let uri = "mongodb://localhost:27017/media_index_db";
    let config = DatabaseSinkConfig::new(uri, None);

    assert_eq!(config.connection_uri, uri);
    assert_eq!(config.database_name, "media_index_db");
    assert_eq!(config.collection_name, "media_assets");
    assert_eq!(config.batch_size, 500);
    assert_eq!(config.timeout_seconds, 10);
    assert_eq!(config.max_retries, 3);
}

#[test]
fn test_database_sink_config_custom_collection() {
    let uri = "mongodb://media_app:secret@127.0.0.1:27017/archive_db?authSource=admin";
    let config = DatabaseSinkConfig::new(uri, Some("custom_items".to_string()));

    assert_eq!(config.connection_uri, uri);
    assert_eq!(config.database_name, "archive_db");
    assert_eq!(config.collection_name, "custom_items");
}

#[test]
fn test_database_sink_config_sanitized_uri() {
    let uri = "mongodb://media_app:sample_mock_secret_xyz@localhost:27017/media_index_db?authSource=media_index_db";
    let config = DatabaseSinkConfig::new(uri, None);

    let sanitized = config.sanitized_uri();
    assert!(
        !sanitized.contains("sample_mock_secret_xyz"),
        "Password must be masked"
    );
    assert!(sanitized
        .contains("media_app:***@localhost:27017/media_index_db?authSource=media_index_db"));
}

#[test]
fn test_database_sink_config_sanitized_uri_no_credentials() {
    let uri = "mongodb://localhost:27017/media_index_db";
    let config = DatabaseSinkConfig::new(uri, None);

    assert_eq!(config.sanitized_uri(), uri);
}

#[test]
fn test_bson_document_deserialization_with_negative_int64_phash() {
    use metadata_extractor::infrastructure::mongodb_sink::bson_document_to_doc;

    let bson_doc = mongodb::bson::doc! {
        "id": "e6f9a0c2-124b-4b13-9a3b-28f9d0c6418b",
        "extraction_status": {
            "status": "Success",
            "warnings": []
        },
        "file_context": {
            "scanned_root": "/test",
            "file_path": "/test/photo.jpg",
            "relative_path": "photo.jpg",
            "file_name": "photo.jpg",
            "file_size_bytes": 1024i64,
            "extension": "jpg",
            "media_category": "Photo",
            "modified_at": "2026-11-10T23:00:00Z"
        },
        "temporal": {
            "timestamp_utc": "2026-11-10T23:00:00Z",
            "iso_timestamp": "2026-11-10T23:00:00+00:00",
            "year": 2026i32,
            "month": 11i32,
            "day": 10i32,
            "hour": 23i32,
            "minute": 0i32,
            "second": 0i32,
            "source": "FileSystemModified"
        },
        "fingerprint": {
            "id": "99312b10-0a12-4f3b-9a4c-88201a43b2cf",
            "file_path": "/test/photo.jpg",
            "file_size_bytes": 1024i64,
            "media_category": "Photo",
            "cryptographic_hash": "c6f605e3b571f6ddcdbfaad3978234193f60c68acc384595face0de869d3e67f",
            "phash": -5690595411479605651i64,
            "rotated_hashes": {
                "hash_90": -1i64,
                "hash_180": -5690595411479605651i64,
                "hash_270": 42i64
            },
            "is_low_entropy": false,
            "fingerprinted_at": "2026-11-10T23:00:00Z"
        }
    };

    let doc = bson_document_to_doc(bson_doc)
        .expect("BSON with negative Int64 perceptual hash must deserialize cleanly");

    let fp = doc.fingerprint.expect("Fingerprint should be present");
    assert_eq!(fp.perceptual_hash, Some(12756148662229945965_u64));
    let rot = fp.rotated_hashes.expect("Rotated hashes should be present");
    assert_eq!(rot.hash_90, u64::MAX);
    assert_eq!(rot.hash_180, 12756148662229945965_u64);
    assert_eq!(rot.hash_270, 42_u64);
}
