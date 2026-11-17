use std::fs::File;
use std::io::Write;
use tempfile::tempdir;

use metadata_extractor::application::transfer_service::UnifiedFileTransferService;
use metadata_extractor::domain::dedup_models::{PathResolution, TransferBufferConfig};
use metadata_extractor::domain::models::MediaCategory;
use metadata_extractor::domain::ports::MediaFingerprinter;
use metadata_extractor::infrastructure::blake3_hasher::Blake3MediaHasher;

#[test]
fn test_build_canonical_relative_path() {
    let path = UnifiedFileTransferService::build_canonical_relative_path(
        2024,
        8,
        15,
        MediaCategory::Photo,
        "sample.jpg",
    );
    assert_eq!(path.to_string_lossy(), "2024/15-Aug-2024/Photos/sample.jpg");

    let raw_path = UnifiedFileTransferService::build_canonical_relative_path(
        2021,
        11,
        30,
        MediaCategory::RawPhoto,
        "raw_01.cr3",
    );
    assert_eq!(
        raw_path.to_string_lossy(),
        "2021/30-Nov-2021/PhotosRaw/raw_01.cr3"
    );

    let vid_path = UnifiedFileTransferService::build_canonical_relative_path(
        2025,
        1,
        1,
        MediaCategory::Video,
        "movie.mp4",
    );
    assert_eq!(
        vid_path.to_string_lossy(),
        "2025/01-Jan-2025/Videos/movie.mp4"
    );
}

#[test]
fn test_resolve_collision_path_available_and_identical() {
    let dir = tempdir().unwrap();
    let hasher = Blake3MediaHasher::new();

    let target = dir.path().join("photo.jpg");
    // Path does not exist yet -> Available
    let res = UnifiedFileTransferService::resolve_collision_path(&target, 100, None, &hasher);
    assert_eq!(res, PathResolution::Available(target.clone()));

    // Create file
    let mut file = File::create(&target).unwrap();
    file.write_all(b"hello world bytes").unwrap();
    let size = 17u64;
    let hash = hasher.compute_cryptographic_hash(&target).unwrap();

    // Matching size and hash -> IdenticalExists
    let res_ident =
        UnifiedFileTransferService::resolve_collision_path(&target, size, Some(&hash), &hasher);
    assert_eq!(res_ident, PathResolution::IdenticalExists(target.clone()));

    // Different size/hash -> CollisionResolved
    let res_collision = UnifiedFileTransferService::resolve_collision_path(
        &target,
        size + 5,
        Some("different_hash"),
        &hasher,
    );
    let expected_col = dir.path().join("photo_1.jpg");
    assert_eq!(
        res_collision,
        PathResolution::CollisionResolved(expected_col)
    );
}

#[tokio::test]
async fn test_stream_copy_verified_success_and_hash_rollback() {
    let dir = tempdir().unwrap();
    let src = dir.path().join("source.jpg");
    let dst = dir.path().join("nested").join("target.jpg");

    let payload = b"Verified binary payload for testing streaming transfer";
    let mut f = File::create(&src).unwrap();
    f.write_all(payload).unwrap();

    let hasher = Blake3MediaHasher::new();
    let exp_hash = hasher.compute_cryptographic_hash(&src).unwrap();

    // 1. Success copy
    let copied = UnifiedFileTransferService::stream_copy_verified(
        &src,
        &dst,
        payload.len() as u64,
        Some(&exp_hash),
        TransferBufferConfig::default(),
    )
    .await
    .unwrap();

    assert_eq!(copied, payload.len() as u64);
    assert!(dst.exists());

    // 2. Hash mismatch rollback
    let dst2 = dir.path().join("nested").join("target2.jpg");
    let result = UnifiedFileTransferService::stream_copy_verified(
        &src,
        &dst2,
        payload.len() as u64,
        Some("bad_hash_value_that_fails_verification"),
        TransferBufferConfig::default(),
    )
    .await;

    assert!(result.is_err());
    assert!(!dst2.exists(), "Corrupt or mismatch target must be purged");
}

#[test]
fn test_find_companion_sidecar() {
    let dir = tempdir().unwrap();

    let img1 = dir.path().join("IMG_1001.HEIC");
    let aae1 = dir.path().join("IMG_1001.AAE");
    std::fs::write(&img1, b"heic_data").unwrap();
    std::fs::write(&aae1, b"plist_data").unwrap();

    // Exact stem match
    let found = UnifiedFileTransferService::find_companion_sidecar(&img1);
    assert_eq!(found, Some(aae1.clone()));

    // Case-insensitive match .aae
    let img2 = dir.path().join("IMG_1002.JPG");
    let aae2 = dir.path().join("IMG_1002.aae");
    std::fs::write(&img2, b"jpg_data").unwrap();
    std::fs::write(&aae2, b"plist_data").unwrap();
    let found2 = UnifiedFileTransferService::find_companion_sidecar(&img2);
    assert_eq!(found2, Some(aae2));

    // Apple edit variant: IMG_E1003.JPG -> IMG_1003.AAE
    let img3 = dir.path().join("IMG_E1003.JPG");
    let aae3 = dir.path().join("IMG_1003.AAE");
    std::fs::write(&img3, b"edited_jpg").unwrap();
    std::fs::write(&aae3, b"plist_data").unwrap();
    let found3 = UnifiedFileTransferService::find_companion_sidecar(&img3);
    assert_eq!(found3, Some(aae3));

    // Video companion: IMG_1004.MOV -> IMG_1004.AAE
    let vid4 = dir.path().join("IMG_1004.MOV");
    let aae4 = dir.path().join("IMG_1004.AAE");
    std::fs::write(&vid4, b"mov_data").unwrap();
    std::fs::write(&aae4, b"plist_data").unwrap();
    let found4 = UnifiedFileTransferService::find_companion_sidecar(&vid4);
    assert_eq!(found4, Some(aae4));

    // Non-existent sidecar returns None
    let img_lone = dir.path().join("IMG_9999.JPG");
    std::fs::write(&img_lone, b"lone").unwrap();
    assert_eq!(
        UnifiedFileTransferService::find_companion_sidecar(&img_lone),
        None
    );

    // DVD Title Set: VTS_01_1.VOB matches both VTS_01_0.IFO and VTS_01_0.BUP
    let vob1 = dir.path().join("VTS_01_1.VOB");
    let ifo1 = dir.path().join("VTS_01_0.IFO");
    let bup1 = dir.path().join("VTS_01_0.BUP");
    std::fs::write(&vob1, b"vob_data").unwrap();
    std::fs::write(&ifo1, b"ifo_data").unwrap();
    std::fs::write(&bup1, b"bup_data").unwrap();

    let sidecars = UnifiedFileTransferService::find_companion_sidecars(&vob1);
    assert_eq!(sidecars.len(), 2);
    assert!(sidecars.contains(&ifo1));
    assert!(sidecars.contains(&bup1));

    // DVD Title Set exact match: VTS_02_1.VOB matches VTS_02_1.IFO
    let vob2 = dir.path().join("VTS_02_1.vob");
    let ifo2 = dir.path().join("VTS_02_1.ifo");
    std::fs::write(&vob2, b"vob2_data").unwrap();
    std::fs::write(&ifo2, b"ifo2_data").unwrap();

    let sidecars2 = UnifiedFileTransferService::find_companion_sidecars(&vob2);
    assert_eq!(sidecars2, vec![ifo2]);
}
