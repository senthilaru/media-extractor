use metadata_extractor::domain::dedup_models::MediaQualityRank;

#[test]
fn test_quality_rank_pixel_count_dominance() {
    // 4000x3000 (12MP) smaller file vs 1920x1080 (2MP) larger file
    let high_res = MediaQualityRank::new(12_000_000, 2_000_000, 3, false, false, 0, false);
    let low_res = MediaQualityRank::new(2_073_600, 8_000_000, 3, true, true, 100, true);

    assert!(high_res > low_res, "Higher resolution must beat lower resolution even if lower resolution has larger file size and drive priority");
}

#[test]
fn test_quality_rank_file_size_tie_breaker_for_equal_resolution() {
    // Both 1920x1080 (2_073_600 pixels), but one is 5MB and other is 500KB
    let high_bitrate = MediaQualityRank::new(2_073_600, 5_000_000, 3, false, false, 0, false);
    let compressed = MediaQualityRank::new(2_073_600, 500_000, 3, true, true, 100, true);

    assert!(high_bitrate > compressed, "Larger file size must win when resolution is identical");
}

#[test]
fn test_quality_rank_metadata_and_drive_tie_breakers() {
    // Identical resolution and size: EXIF presence wins
    let with_exif = MediaQualityRank::new(2_073_600, 1_000_000, 3, true, true, 0, false);
    let no_exif = MediaQualityRank::new(2_073_600, 1_000_000, 3, false, false, 0, false);
    assert!(with_exif > no_exif);

    // Identical resolution, size, and metadata: higher drive priority wins
    let master_drive = MediaQualityRank::new(2_073_600, 1_000_000, 3, true, true, 100, true);
    let staging_drive = MediaQualityRank::new(2_073_600, 1_000_000, 3, true, true, 10, false);
    assert!(master_drive > staging_drive);
}
