use std::fs::File;
use std::io::Write;
use tempfile::tempdir;

use metadata_extractor::application::transfer_service::{
    InFlightStreamHasher, UnifiedFileTransferService,
};
use metadata_extractor::domain::dedup_models::TransferBufferConfig;
use metadata_extractor::domain::ports::MediaFingerprinter;
use metadata_extractor::infrastructure::blake3_hasher::Blake3MediaHasher;

#[test]
fn test_transfer_buffer_config_bounds_and_parsing() {
    let default_cfg = TransferBufferConfig::default();
    assert_eq!(default_cfg.buffer_size_bytes, 8 * 1024 * 1024);
    assert!(default_cfg.in_flight_hash);
    assert!(!default_cfg.recheck_disk_hash);

    // Clamping behavior
    let clamped_low = TransferBufferConfig::new(1024, true, false);
    assert_eq!(clamped_low.buffer_size_bytes, 64 * 1024);

    let clamped_high = TransferBufferConfig::new(100 * 1024 * 1024, true, false);
    assert_eq!(clamped_high.buffer_size_bytes, 32 * 1024 * 1024);

    // Parsing human-readable strings
    assert_eq!(
        TransferBufferConfig::parse_size_str("4M").unwrap(),
        4 * 1024 * 1024
    );
    assert_eq!(
        TransferBufferConfig::parse_size_str("8MB").unwrap(),
        8 * 1024 * 1024
    );
    assert_eq!(
        TransferBufferConfig::parse_size_str("16m").unwrap(),
        16 * 1024 * 1024
    );
    assert_eq!(
        TransferBufferConfig::parse_size_str("64K").unwrap(),
        64 * 1024
    );
    assert_eq!(
        TransferBufferConfig::parse_size_str("512kb").unwrap(),
        512 * 1024
    );
    assert_eq!(
        TransferBufferConfig::parse_size_str("1048576").unwrap(),
        1048576
    );

    // Exceeding bounds or invalid
    assert!(TransferBufferConfig::parse_size_str("1G").is_err());
    assert!(TransferBufferConfig::parse_size_str("64M").is_err());
    assert!(TransferBufferConfig::parse_size_str("invalid").is_err());
}

#[test]
fn test_effective_buffer_size_bounding() {
    let cfg = TransferBufferConfig::default(); // 8 MB

    // 0 bytes bounded to minimum (64 KB)
    assert_eq!(cfg.effective_buffer_size(0), 64 * 1024);

    // Small file below 64KB bounded to minimum (64 KB)
    assert_eq!(cfg.effective_buffer_size(1024), 64 * 1024);

    // File between 64 KB and 8 MB dynamically bounded to file size
    assert_eq!(cfg.effective_buffer_size(256 * 1024), 256 * 1024);
    assert_eq!(cfg.effective_buffer_size(3 * 1024 * 1024), 3 * 1024 * 1024);

    // File larger than 8 MB capped at configured buffer size (8 MB)
    assert_eq!(cfg.effective_buffer_size(50 * 1024 * 1024), 8 * 1024 * 1024);
}

#[test]
fn test_in_flight_stream_hasher_accuracy_and_verification() {
    let payload = b"Hello, continuous streaming in-flight verification with BLAKE3!";
    let blake3_expected = blake3::hash(payload).to_hex().to_string();

    let mut hasher = InFlightStreamHasher::new(Some(&blake3_expected), payload.len() as u64);
    // Feed in chunks
    hasher.update(&payload[..15]);
    hasher.update(&payload[15..30]);
    hasher.update(&payload[30..]);

    let finalized = hasher.finalize_hex();
    assert_eq!(finalized, blake3_expected);

    // Verification succeeds
    assert!(hasher.finalize_and_verify().is_ok());

    // Mismatched expected hash fails
    let hasher_bad = InFlightStreamHasher::new(
        Some("0000000000000000000000000000000000000000000000000000000000000000"),
        payload.len() as u64,
    );
    assert!(hasher_bad.finalize_and_verify().is_err());
}

#[tokio::test]
async fn test_in_flight_stream_hasher_verifies_sample_digest_for_large_files() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("large_sample_test.bin");
    let dst = dir.path().join("large_sample_dest.bin");

    // 256 KB file (greater than 32KB sample chunk boundary)
    let payload = vec![0x33u8; 256 * 1024];
    {
        let mut f = File::create(&src).unwrap();
        f.write_all(&payload).unwrap();
    }

    let fingerprinter = Blake3MediaHasher::new();
    let sample_digest = fingerprinter.compute_sample_digest(&src).unwrap();
    let full_hash = fingerprinter.compute_cryptographic_hash(&src).unwrap();
    assert_ne!(sample_digest, full_hash);

    // 1. Verify that InFlightStreamHasher succeeds when expected_hash is the sample digest
    let mut hasher = InFlightStreamHasher::new(Some(&sample_digest), payload.len() as u64);
    for chunk in payload.chunks(64 * 1024) {
        hasher.update(chunk);
    }
    assert_eq!(hasher.finalize_sample_digest_hex(), sample_digest);
    assert_eq!(hasher.finalize_hex(), full_hash);
    assert!(hasher.finalize_and_verify().is_ok());

    // 2. Verify stream_copy_verified succeeds when expected_hash is the sample digest
    let config = TransferBufferConfig::default();
    let transferred = UnifiedFileTransferService::stream_copy_verified(
        &src,
        &dst,
        payload.len() as u64,
        Some(&sample_digest),
        config,
    )
    .await
    .unwrap();

    assert_eq!(transferred, payload.len() as u64);
    assert!(dst.exists());
}

#[tokio::test]
async fn test_stream_copy_verified_with_8mb_buffer_and_small_files() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("source_8mb.bin");
    let dst = dir.path().join("dest_8mb.bin");

    // 9 MB test file to trigger multiple chunk iterations
    let payload = vec![0xABu8; 9 * 1024 * 1024];
    {
        let mut f = File::create(&src).unwrap();
        f.write_all(&payload).unwrap();
    }

    let fingerprinter = Blake3MediaHasher::new();
    let expected_hash = fingerprinter.compute_cryptographic_hash(&src).unwrap();

    let config = TransferBufferConfig::default(); // 8 MB buffer
    let transferred = UnifiedFileTransferService::stream_copy_verified(
        &src,
        &dst,
        payload.len() as u64,
        Some(&expected_hash),
        config,
    )
    .await
    .unwrap();

    assert_eq!(transferred, payload.len() as u64);
    assert!(dst.exists());

    // Verify bitwise match
    let copied_hash = fingerprinter.compute_cryptographic_hash(&dst).unwrap();
    assert_eq!(copied_hash, expected_hash);
}

#[tokio::test]
async fn test_in_flight_hash_mismatch_rollback() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("source.dat");
    let dst = dir.path().join("corrupted_target.dat");

    let payload = b"Cryptographically protected asset bytes";
    {
        let mut f = File::create(&src).unwrap();
        f.write_all(payload).unwrap();
    }

    let config = TransferBufferConfig::default();
    let wrong_hash = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";

    let result = UnifiedFileTransferService::stream_copy_verified(
        &src,
        &dst,
        payload.len() as u64,
        Some(wrong_hash),
        config,
    )
    .await;

    assert!(result.is_err());
    assert!(
        !dst.exists(),
        "Target file must be unlinked/purged on in-flight hash mismatch"
    );
}

#[tokio::test]
async fn test_recheck_disk_hash_mode() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("source_recheck.dat");
    let dst = dir.path().join("dest_recheck.dat");

    let payload = b"Payload for secondary disk re-read verification pass";
    {
        let mut f = File::create(&src).unwrap();
        f.write_all(payload).unwrap();
    }

    let fingerprinter = Blake3MediaHasher::new();
    let expected_hash = fingerprinter.compute_cryptographic_hash(&src).unwrap();

    // Enable recheck_disk_hash
    let config = TransferBufferConfig {
        recheck_disk_hash: true,
        ..Default::default()
    };

    let transferred = UnifiedFileTransferService::stream_copy_verified(
        &src,
        &dst,
        payload.len() as u64,
        Some(&expected_hash),
        config,
    )
    .await
    .unwrap();

    assert_eq!(transferred, payload.len() as u64);
    assert!(dst.exists());
}

#[tokio::test]
async fn test_size_mismatch_rollback() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("source_size.dat");
    let dst = dir.path().join("dest_size_fail.dat");

    let payload = b"Actual byte content is 38 bytes long..";
    {
        let mut f = File::create(&src).unwrap();
        f.write_all(payload).unwrap();
    }

    let config = TransferBufferConfig::default();
    let result = UnifiedFileTransferService::stream_copy_verified(
        &src,
        &dst,
        payload.len() as u64 + 100, // Intentionally wrong expected size
        None,
        config,
    )
    .await;

    assert!(result.is_err());
    assert!(
        !dst.exists(),
        "Target file must be unlinked/purged on size mismatch"
    );
}
