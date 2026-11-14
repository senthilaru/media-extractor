use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use metadata_extractor::infrastructure::drive_spindle_lock::DriveSpindleLockManager;

#[tokio::test]
async fn test_detect_spindle_root_conventions() {
    let vol_path = Path::new("/Volumes/SP-1/SPSA/media/master/2026/01/photo.jpg");
    assert_eq!(
        DriveSpindleLockManager::detect_spindle_root(vol_path),
        PathBuf::from("/Volumes/SP-1")
    );

    let mnt_path = Path::new("/mnt/cold_archive/photos/2025/test.cr3");
    assert_eq!(
        DriveSpindleLockManager::detect_spindle_root(mnt_path),
        PathBuf::from("/mnt/cold_archive")
    );

    let media_path = Path::new("/media/operator/external_drive/videos/clip.mp4");
    assert_eq!(
        DriveSpindleLockManager::detect_spindle_root(media_path),
        PathBuf::from("/media/operator/external_drive")
    );
}

#[tokio::test]
async fn test_spindle_serialization_same_spindle() {
    let manager = DriveSpindleLockManager::new();
    let drive_root = PathBuf::from("/Volumes/SP-1");
    manager.register_mount_point(&drive_root).await;

    let in_flight_count = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();
    for i in 0..5 {
        let mgr = manager.clone();
        let inflight = in_flight_count.clone();
        let max_conc = max_concurrent.clone();
        let file_path = drive_root.join(format!("file_{}.mp4", i));

        handles.push(tokio::spawn(async move {
            let _guard = mgr.acquire_spindle_lock(&file_path).await;
            let current = inflight.fetch_add(1, Ordering::SeqCst) + 1;
            max_conc.fetch_max(current, Ordering::SeqCst);

            // Simulate file transfer I/O duration
            sleep(Duration::from_millis(20)).await;

            inflight.fetch_sub(1, Ordering::SeqCst);
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    // Exactly 1 concurrent operation allowed per physical spindle
    assert_eq!(max_concurrent.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_inter_drive_concurrency_distinct_spindles() {
    let manager = DriveSpindleLockManager::new();
    let drive_a = PathBuf::from("/Volumes/SP-1");
    let drive_b = PathBuf::from("/Volumes/SP-2");
    manager.register_mount_point(&drive_a).await;
    manager.register_mount_point(&drive_b).await;

    let both_active = Arc::new(AtomicBool::new(false));
    let active_a = Arc::new(AtomicBool::new(false));
    let active_b = Arc::new(AtomicBool::new(false));

    let mgr1 = manager.clone();
    let act_a1 = active_a.clone();
    let act_b1 = active_b.clone();
    let both1 = both_active.clone();
    let h1 = tokio::spawn(async move {
        let _guard = mgr1.acquire_spindle_lock(&drive_a.join("a.mov")).await;
        act_a1.store(true, Ordering::SeqCst);
        for _ in 0..10 {
            if act_b1.load(Ordering::SeqCst) {
                both1.store(true, Ordering::SeqCst);
            }
            sleep(Duration::from_millis(10)).await;
        }
        act_a1.store(false, Ordering::SeqCst);
    });

    let mgr2 = manager.clone();
    let act_a2 = active_a.clone();
    let act_b2 = active_b.clone();
    let both2 = both_active.clone();
    let h2 = tokio::spawn(async move {
        let _guard = mgr2.acquire_spindle_lock(&drive_b.join("b.mov")).await;
        act_b2.store(true, Ordering::SeqCst);
        for _ in 0..10 {
            if act_a2.load(Ordering::SeqCst) {
                both2.store(true, Ordering::SeqCst);
            }
            sleep(Duration::from_millis(10)).await;
        }
        act_b2.store(false, Ordering::SeqCst);
    });

    h1.await.unwrap();
    h2.await.unwrap();

    // Distinct spindles run concurrently without blocking each other
    assert!(both_active.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_deadlock_free_bidirectional_transfers() {
    let manager = DriveSpindleLockManager::new();
    let drive_a = PathBuf::from("/Volumes/SP-1");
    let drive_b = PathBuf::from("/Volumes/SP-2");
    manager.register_mount_point(&drive_a).await;
    manager.register_mount_point(&drive_b).await;

    let mgr1 = manager.clone();
    let da1 = drive_a.clone();
    let db1 = drive_b.clone();
    let h1 = tokio::spawn(async move {
        for i in 0..10 {
            let _guard = mgr1
                .acquire_transfer_locks(
                    &da1.join(format!("src_{}.mp4", i)),
                    &db1.join(format!("dst_{}.mp4", i)),
                )
                .await;
            sleep(Duration::from_millis(5)).await;
        }
    });

    let mgr2 = manager.clone();
    let da2 = drive_a.clone();
    let db2 = drive_b.clone();
    let h2 = tokio::spawn(async move {
        for i in 0..10 {
            let _guard = mgr2
                .acquire_transfer_locks(
                    &db2.join(format!("src_{}.mp4", i)),
                    &da2.join(format!("dst_{}.mp4", i)),
                )
                .await;
            sleep(Duration::from_millis(5)).await;
        }
    });

    // Neither task must deadlock
    let res = tokio::time::timeout(Duration::from_secs(5), async {
        let (r1, r2) = tokio::join!(h1, h2);
        r1.unwrap();
        r2.unwrap();
    })
    .await;

    assert!(
        res.is_ok(),
        "Bidirectional cross-drive transfers must not deadlock"
    );
}
