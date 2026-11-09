use metadata_extractor::domain::errors::StorageError;
use metadata_extractor::domain::metadata_models::{AppProfile, DatabaseSinkConfig};
use std::str::FromStr;

#[test]
fn test_app_profile_parsing() {
    assert_eq!(AppProfile::from_str("local").unwrap(), AppProfile::Local);
    assert_eq!(AppProfile::from_str("LOCAL").unwrap(), AppProfile::Local);
    assert_eq!(AppProfile::from_str("dev").unwrap(), AppProfile::Local);
    assert_eq!(
        AppProfile::from_str("production").unwrap(),
        AppProfile::Production
    );
    assert_eq!(
        AppProfile::from_str("PROD").unwrap(),
        AppProfile::Production
    );
    assert!(AppProfile::from_str("invalid_profile").is_err());
}

#[test]
fn test_secret_interpolation_placeholders() {
    let uri = "mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db?authSource=media_index_db";
    let interpolated = DatabaseSinkConfig::interpolate_secret(uri, Some("secret_pass_123"));
    assert_eq!(
        interpolated,
        "mongodb://media_app:secret_pass_123@localhost:27017/media_index_db?authSource=media_index_db"
    );

    let uri2 = "mongodb://media_app:<password>@localhost:27017/media_index_db";
    let interpolated2 = DatabaseSinkConfig::interpolate_secret(uri2, Some("secret_pass_123"));
    assert_eq!(
        interpolated2,
        "mongodb://media_app:secret_pass_123@localhost:27017/media_index_db"
    );

    let uri3 = "mongodb://media_app:${MONGODB_PASSWORD}@localhost:27017/media_index_db";
    let interpolated3 = DatabaseSinkConfig::interpolate_secret(uri3, Some("secret_pass_123"));
    assert_eq!(
        interpolated3,
        "mongodb://media_app:secret_pass_123@localhost:27017/media_index_db"
    );
}

#[test]
fn test_secret_interpolation_url_encoding() {
    let uri = "mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db";
    // Password with special characters: '@', ':', '/', '%'
    let interpolated = DatabaseSinkConfig::interpolate_secret(uri, Some("p@ss:w/rd%99"));
    assert_eq!(
        interpolated,
        "mongodb://media_app:p%40ss%3Aw%2Frd%2599@localhost:27017/media_index_db"
    );
}

#[test]
fn test_production_profile_validation() {
    // Missing password placeholder in production must fail
    let unpopulated_config = DatabaseSinkConfig::new(
        "mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db",
        None,
    );
    let res = unpopulated_config.validate_for_profile(AppProfile::Production);
    assert!(res.is_err());
    match res.unwrap_err() {
        StorageError::ConfigurationError(msg) => {
            assert!(msg.contains("unresolved password placeholder"));
        }
        other => panic!("Expected ConfigurationError, got: {:?}", other),
    }

    // Insecure default password in production must fail
    let insecure_config = DatabaseSinkConfig::new(
        "mongodb://media_app:password@localhost:27017/media_index_db",
        None,
    );
    assert!(insecure_config
        .validate_for_profile(AppProfile::Production)
        .is_err());

    // Valid credentials in production must pass
    let valid_config = DatabaseSinkConfig::new(
        "mongodb://media_app:Strong_Auth_Token_987@localhost:27017/media_index_db?authSource=media_index_db",
        None,
    );
    assert!(valid_config
        .validate_for_profile(AppProfile::Production)
        .is_ok());

    // Local profile with relaxed credentials must pass
    assert!(insecure_config
        .validate_for_profile(AppProfile::Local)
        .is_ok());
}

#[test]
fn test_sanitized_uri_masks_interpolated_passwords() {
    let uri = DatabaseSinkConfig::interpolate_secret(
        "mongodb://media_app:<MONGODB_PASSWORD>@localhost:27017/media_index_db",
        Some("super_secret_password_value"),
    );
    let config = DatabaseSinkConfig::new(uri, None);
    let sanitized = config.sanitized_uri();
    assert!(!sanitized.contains("super_secret_password_value"));
    assert!(sanitized.contains("mongodb://media_app:***@localhost:27017/media_index_db"));
}

#[test]
fn test_security_audit_no_plaintext_passwords_in_config_display() {
    let secret = "P@ssw0rd_Super_Sensitive_123!";
    let raw_uri = "mongodb://admin:<MONGODB_PASSWORD>@cluster.mongodb.net/prod_db";
    let interpolated = DatabaseSinkConfig::interpolate_secret(raw_uri, Some(secret));
    let config = DatabaseSinkConfig::new(&interpolated, Some("assets".into()));

    // Verify sanitized URI masks the secret
    let sanitized = config.sanitized_uri();
    assert!(!sanitized.contains(secret));
    assert!(sanitized.contains("admin:***@"));

    // Verify static helper masks it as well
    let static_sanitized = DatabaseSinkConfig::sanitize_uri_str(&interpolated);
    assert!(!static_sanitized.contains(secret));
    assert!(static_sanitized.contains("admin:***@"));
}
