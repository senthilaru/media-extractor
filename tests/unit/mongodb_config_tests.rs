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
