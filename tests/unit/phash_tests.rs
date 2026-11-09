use std::path::PathBuf;

use metadata_extractor::domain::dedup_models::FingerprintConfig;
use metadata_extractor::domain::models::{DiscoveredMediaItem, MediaCategory};
use metadata_extractor::infrastructure::blake3_hasher::Blake3MediaHasher;
use metadata_extractor::infrastructure::phash::DctPerceptualHasher;
use metadata_extractor::MediaFingerprinter;

fn fixture_path(filename: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("dedup_samples")
        .join(filename)
}

fn discovered_item(path: PathBuf) -> DiscoveredMediaItem {
    DiscoveredMediaItem {
        path,
        file_size_bytes: 1000,
        modified_at: None,
        category: MediaCategory::Photo,
        extension: "jpg".to_string(),
    }
}

#[test]
fn test_hamming_distance() {
    assert_eq!(DctPerceptualHasher::hamming_distance(0, 0), 0);
    assert_eq!(DctPerceptualHasher::hamming_distance(0, u64::MAX), 64);
    assert_eq!(DctPerceptualHasher::hamming_distance(0b1010, 0b0010), 1);
    assert_eq!(DctPerceptualHasher::hamming_distance(0b1111, 0b0000), 4);
}

#[test]
fn test_low_entropy_detection() {
    let black = [0u8; 1024];
    assert!(DctPerceptualHasher::is_low_entropy(&black));

    let white = [255u8; 1024];
    assert!(DctPerceptualHasher::is_low_entropy(&white));

    // Checkerboard high entropy
    let mut pattern = [0u8; 1024];
    for (i, item) in pattern.iter_mut().enumerate() {
        *item = if (i % 2) == 0 { 255 } else { 0 };
    }
    assert!(!DctPerceptualHasher::is_low_entropy(&pattern));
}

#[test]
fn test_rotation_matrix_transforms() {
    let mut matrix = [0u8; 1024];
    // Put a marker at top-left (r=0, c=0)
    matrix[0] = 200;

    let rot90 = DctPerceptualHasher::rotate_90(&matrix);
    // (0,0) in original should map to (0, 31) in 90-degree clockwise: r'=c=0, c'=31-r=31
    assert_eq!(rot90[31], 200);

    let rot180 = DctPerceptualHasher::rotate_180(&matrix);
    // (0,0) should map to (31, 31): 1023 - 0 = 1023
    assert_eq!(rot180[31 * 32 + 31], 200);

    let rot270 = DctPerceptualHasher::rotate_270(&matrix);
    // (0,0) should map to (31, 0): r'=31-c=31, c'=r=0
    assert_eq!(rot270[31 * 32], 200);
}

#[test]
fn test_phash_thumbnail_scale_invariance() {
    let hasher = Blake3MediaHasher::new();
    let config = FingerprintConfig {
        enable_perceptual: true,
        enable_rotation: true,
    };

    let p_orig = fixture_path("photo_original.jpg");
    let p_thumb = fixture_path("photo_thumbnail.jpg");

    let fp_orig = hasher
        .fingerprint_file(&discovered_item(p_orig), None, &config)
        .expect("original fingerprint should succeed");
    let fp_thumb = hasher
        .fingerprint_file(&discovered_item(p_thumb), None, &config)
        .expect("thumbnail fingerprint should succeed");

    assert!(!fp_orig.is_low_entropy);
    assert!(!fp_thumb.is_low_entropy);

    let orig_phash = fp_orig.perceptual_hash.expect("orig has phash");
    let thumb_phash = fp_thumb.perceptual_hash.expect("thumb has phash");

    let dist = DctPerceptualHasher::hamming_distance(orig_phash, thumb_phash);
    // Perceptual hash should be very close (Hamming distance <= 10)
    assert!(
        dist <= 10,
        "Expected Hamming distance between original and thumbnail <= 10, got {}",
        dist
    );
}

#[test]
fn test_phash_rotation_invariance() {
    let hasher = Blake3MediaHasher::new();
    let config = FingerprintConfig {
        enable_perceptual: true,
        enable_rotation: true,
    };

    let p_orig = fixture_path("photo_original.jpg");
    let p_rot90 = fixture_path("photo_rot90.jpg");
    let p_rot180 = fixture_path("photo_rot180.jpg");

    let fp_orig = hasher
        .fingerprint_file(&discovered_item(p_orig), None, &config)
        .expect("original fingerprint should succeed");
    let fp_rot90 = hasher
        .fingerprint_file(&discovered_item(p_rot90), None, &config)
        .expect("rot90 fingerprint should succeed");
    let fp_rot180 = hasher
        .fingerprint_file(&discovered_item(p_rot180), None, &config)
        .expect("rot180 fingerprint should succeed");

    let orig_rot = fp_orig.rotated_hashes.expect("orig has rotated hashes");
    let rot90_phash = fp_rot90.perceptual_hash.expect("rot90 has phash");
    let rot180_phash = fp_rot180.perceptual_hash.expect("rot180 has phash");

    // Note: PIL's img.rotate(90) rotates counter-clockwise 90 degrees, which corresponds to 270 degrees clockwise.
    let dist_ccw90 = DctPerceptualHasher::hamming_distance(orig_rot.hash_270, rot90_phash);
    assert!(
        dist_ccw90 <= 5,
        "Expected Hamming distance between orig.hash_270 and rot90 image <= 5, got {}",
        dist_ccw90
    );

    let dist_180 = DctPerceptualHasher::hamming_distance(orig_rot.hash_180, rot180_phash);
    assert!(
        dist_180 <= 5,
        "Expected Hamming distance between orig.hash_180 and rot180 image <= 5, got {}",
        dist_180
    );

    // Also verify min_hamming distance helper
    assert!(fp_orig.min_hamming_distance(&fp_rot90).unwrap() <= 5);
    assert!(fp_orig.min_hamming_distance(&fp_rot180).unwrap() <= 5);
}

#[test]
fn test_phash_blank_images_marked_low_entropy() {
    let hasher = Blake3MediaHasher::new();
    let config = FingerprintConfig {
        enable_perceptual: true,
        enable_rotation: true,
    };

    let p_black = fixture_path("blank_black.jpg");
    let p_white = fixture_path("blank_white.jpg");

    let fp_black = hasher
        .fingerprint_file(&discovered_item(p_black), None, &config)
        .expect("blank black fingerprint should succeed");
    let fp_white = hasher
        .fingerprint_file(&discovered_item(p_white), None, &config)
        .expect("blank white fingerprint should succeed");

    assert!(fp_black.is_low_entropy, "blank_black should be low entropy");
    assert!(fp_white.is_low_entropy, "blank_white should be low entropy");
}

#[test]
fn test_phash_distinct_images_high_distance() {
    let hasher = Blake3MediaHasher::new();
    let config = FingerprintConfig {
        enable_perceptual: true,
        enable_rotation: true,
    };

    let p_orig = fixture_path("photo_original.jpg");
    let p_distinct = fixture_path("photo_distinct.jpg");

    let fp_orig = hasher
        .fingerprint_file(&discovered_item(p_orig), None, &config)
        .expect("original fingerprint should succeed");
    let fp_distinct = hasher
        .fingerprint_file(&discovered_item(p_distinct), None, &config)
        .expect("distinct fingerprint should succeed");

    let orig_phash = fp_orig.perceptual_hash.expect("orig has phash");
    let distinct_phash = fp_distinct.perceptual_hash.expect("distinct has phash");

    let dist = DctPerceptualHasher::hamming_distance(orig_phash, distinct_phash);
    assert!(
        dist > 10,
        "Expected distinct images to have Hamming distance > 10, got {}",
        dist
    );
}
