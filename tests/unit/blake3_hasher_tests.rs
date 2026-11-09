use std::path::PathBuf;
use tempfile::NamedTempFile;

use metadata_extractor::domain::dedup_models::FingerprintConfig;
use metadata_extractor::domain::errors::FingerprintError;
use metadata_extractor::domain::models::{DiscoveredMediaItem, MediaCategory};
use metadata_extractor::domain::ports::MediaFingerprinter;
use metadata_extractor::infrastructure::blake3_hasher::Blake3MediaHasher;

#[test]
fn test_blake3_identical_files_produce_matching_digest() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let orig = PathBuf::from(manifest_dir).join("tests/fixtures/dedup_samples/photo_original.jpg");
    let copy =
        PathBuf::from(manifest_dir).join("tests/fixtures/dedup_samples/photo_exact_copy.jpg");

    assert!(orig.exists() && copy.exists(), "Sample fixtures must exist");

    let hasher = Blake3MediaHasher::new();
    let hash_orig = hasher
        .compute_cryptographic_hash(&orig)
        .expect("Hashing original must succeed");
    let hash_copy = hasher
        .compute_cryptographic_hash(&copy)
        .expect("Hashing copy must succeed");

    assert_eq!(hash_orig.len(), 64);
    assert_eq!(
        hash_orig, hash_copy,
        "Exact duplicates must yield identical BLAKE3 hashes"
    );
}

#[test]
fn test_blake3_distinct_files_produce_different_digests() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let orig = PathBuf::from(manifest_dir).join("tests/fixtures/dedup_samples/photo_original.jpg");
    let distinct =
        PathBuf::from(manifest_dir).join("tests/fixtures/dedup_samples/photo_distinct.jpg");

    let hasher = Blake3MediaHasher::new();
    let hash_orig = hasher.compute_cryptographic_hash(&orig).unwrap();
    let hash_distinct = hasher.compute_cryptographic_hash(&distinct).unwrap();

    assert_ne!(hash_orig, hash_distinct);
}

#[test]
fn test_tier2_sample_digest_behavior() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let orig = PathBuf::from(manifest_dir).join("tests/fixtures/dedup_samples/photo_original.jpg");
    let copy =
        PathBuf::from(manifest_dir).join("tests/fixtures/dedup_samples/photo_exact_copy.jpg");
    let diff = PathBuf::from(manifest_dir)
        .join("tests/fixtures/dedup_samples/photo_same_size_diff_content.jpg");

    let hasher = Blake3MediaHasher::new();
    let sample_orig = hasher.compute_sample_digest(&orig).unwrap();
    let sample_copy = hasher.compute_sample_digest(&copy).unwrap();
    let sample_diff = hasher.compute_sample_digest(&diff).unwrap();

    assert_eq!(sample_orig, sample_copy);
    assert_ne!(
        sample_orig, sample_diff,
        "Same-size file with mutated bytes must produce distinct sample digest"
    );
}

#[test]
fn test_zero_byte_file_error() {
    let temp = NamedTempFile::new().unwrap();
    let hasher = Blake3MediaHasher::new();
    let res = hasher.compute_sample_digest(temp.path());

    assert!(matches!(res, Err(FingerprintError::ZeroByteFile(_))));
}

#[test]
fn test_file_not_found_error() {
    let hasher = Blake3MediaHasher::new();
    let res = hasher.compute_cryptographic_hash(&PathBuf::from("/non/existent/file.bin"));

    assert!(matches!(res, Err(FingerprintError::FileNotFound(_))));
}

#[test]
fn test_fingerprint_file_generation() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let orig = PathBuf::from(manifest_dir).join("tests/fixtures/dedup_samples/photo_original.jpg");
    let size = std::fs::metadata(&orig).unwrap().len();

    let item = DiscoveredMediaItem {
        path: orig.clone(),
        file_size_bytes: size,
        modified_at: None,
        category: MediaCategory::Photo,
        extension: "jpg".to_string(),
    };

    let hasher = Blake3MediaHasher::new();
    let fp = hasher
        .fingerprint_file(&item, None, &FingerprintConfig::default())
        .expect("Fingerprint generation must succeed");

    assert_eq!(fp.file_path, orig);
    assert_eq!(fp.file_size_bytes, size);
    assert!(fp.sample_digest.is_some());
    assert_eq!(fp.cryptographic_hash.len(), 64);
}
