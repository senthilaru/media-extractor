use std::path::Path;
use std::sync::Mutex;

use metadata_extractor::domain::dedup_models::CanonicalPathStatus;
use metadata_extractor::infrastructure::drives_config::{CanonicalPathMatcher, DrivesConfigLoader};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn acquire_env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

const SAMPLE_YAML: &str = r#"
version: "1.0"
groups:
  primary:
    priority: 100
    description: "Primary master drive group"
  replica1:
    priority: 80
    description: "Replica 1 group"

drives:
  - id: "primary_main"
    group: "primary"
    mount_path: "/Volumes/SP-1"
    master_path: "/Volumes/SP-1/SPSA/media/master"

  - id: "replica_main"
    group: "replica1"
    mount_path: "/Volumes/SP-2"
    master_path: "/Volumes/SP-2/SPSA/media/master"
"#;

#[test]
fn test_synthesize_from_env_success() {
    let _lock = acquire_env_lock();
    std::env::set_var("DRIVE_PRIMARY_MOUNT", "/tmp/mock_primary");
    std::env::set_var("DRIVE_PRIMARY_MASTER", "/tmp/mock_primary/custom/master");
    std::env::set_var("DRIVE_REPLICA1_MOUNT", "/tmp/mock_replica1");
    std::env::set_var("DRIVE_REPLICA2_MOUNT", "/tmp/mock_replica2");

    let config = DrivesConfigLoader::synthesize_from_env()
        .expect("Should synthesize valid DriveConfiguration from environment");

    assert_eq!(config.version, "1.0");
    assert_eq!(config.groups["primary"].priority, 100);
    assert_eq!(config.groups["replica1"].priority, 80);
    assert_eq!(config.groups["replica2"].priority, 70);

    assert_eq!(config.drives.len(), 3);
    assert_eq!(config.drives[0].id, "primary_main");
    assert_eq!(
        config.drives[0].master_path,
        Path::new("/tmp/mock_primary/custom/master")
    );
    assert_eq!(
        config.drives[1].master_path,
        Path::new("/tmp/mock_replica1/SPSA/media/master")
    );
    assert_eq!(
        config.drives[2].master_path,
        Path::new("/tmp/mock_replica2/SPSA/media/master")
    );

    // Clean up
    std::env::remove_var("DRIVE_PRIMARY_MOUNT");
    std::env::remove_var("DRIVE_PRIMARY_MASTER");
    std::env::remove_var("DRIVE_REPLICA1_MOUNT");
    std::env::remove_var("DRIVE_REPLICA2_MOUNT");
}

#[test]
fn test_synthesize_from_env_missing_primary() {
    let _lock = acquire_env_lock();
    std::env::remove_var("DRIVE_PRIMARY_MOUNT");
    std::env::remove_var("PRIMARY_MOUNT_PATH");

    let res = DrivesConfigLoader::synthesize_from_env();
    assert!(res.is_err(), "Must fail when DRIVE_PRIMARY_MOUNT is absent");
    let err_str = res.unwrap_err().to_string();
    assert!(
        err_str.contains("DRIVE_PRIMARY_MOUNT"),
        "Error message should mention DRIVE_PRIMARY_MOUNT: {}",
        err_str
    );
}

#[test]
fn test_load_from_env_or_path_resolution() {
    let _lock = acquire_env_lock();
    // 1. Missing explicit file returns error
    let missing_path = Path::new("/tmp/non_existent_drives_file_12345.yaml");
    let res = DrivesConfigLoader::load_from_env_or_path(Some(missing_path));
    assert!(res.is_err());

    // 2. None fallback to env
    std::env::set_var("DRIVE_PRIMARY_MOUNT", "/tmp/mock_primary_auto");
    let auto_res = DrivesConfigLoader::load_from_env_or_path(None);
    assert!(
        auto_res.is_ok(),
        "Should fall back to env when no path provided"
    );
    assert_eq!(
        auto_res.unwrap().drives[0].mount_path,
        Path::new("/tmp/mock_primary_auto")
    );
    std::env::remove_var("DRIVE_PRIMARY_MOUNT");
}

#[test]
fn test_interpolate_env_vars_in_yaml_loading() {
    let _lock = acquire_env_lock();
    std::env::set_var("MOCK_ENV_DRIVE", "/Volumes/DynamicDrive");

    let yaml = r#"
version: "1.0"
groups:
  primary: { priority: 100 }
drives:
  - id: "primary"
    group: "primary"
    mount_path: "${MOCK_ENV_DRIVE}"
    master_path: "${MOCK_ENV_DRIVE}/master"
"#;

    let config = DrivesConfigLoader::load_from_str(yaml, Some("yaml"))
        .expect("Interpolated yaml should parse cleanly");
    assert_eq!(
        config.drives[0].mount_path,
        Path::new("/Volumes/DynamicDrive")
    );
    assert_eq!(
        config.drives[0].master_path,
        Path::new("/Volumes/DynamicDrive/master")
    );

    std::env::remove_var("MOCK_ENV_DRIVE");
}

#[test]
fn test_drives_example_yaml_loads_cleanly() {
    let _lock = acquire_env_lock();
    std::env::remove_var("DRIVE_PRIMARY_MOUNT");
    std::env::remove_var("DRIVE_PRIMARY_MASTER");
    std::env::remove_var("DRIVE_REPLICA1_MOUNT");
    std::env::remove_var("DRIVE_REPLICA1_MASTER");
    std::env::remove_var("DRIVE_REPLICA2_MOUNT");
    std::env::remove_var("DRIVE_REPLICA2_MASTER");
    std::env::remove_var("DRIVE_STAGING_MOUNT");
    std::env::remove_var("DRIVE_STAGING_MASTER");

    let example_path = Path::new("drives.example.yaml");
    let config = DrivesConfigLoader::load_from_path(example_path)
        .expect("drives.example.yaml must load and validate cleanly");

    assert_eq!(config.version, "1.0");
    assert_eq!(config.groups["primary"].priority, 100);
    assert_eq!(config.groups["replica1"].priority, 80);
    assert_eq!(config.groups["replica2"].priority, 70);
    assert_eq!(config.groups["staging"].priority, 20);

    assert_eq!(config.drives.len(), 4);
    let primary = config
        .drives
        .iter()
        .find(|d| d.id == "primary_main")
        .unwrap();
    assert_eq!(primary.group, "primary");
    assert_eq!(primary.mount_path, Path::new("/Volumes/DummyPrimary-1"));
    assert_eq!(
        primary.master_path,
        Path::new("/Volumes/DummyPrimary-1/SPSA/media/master")
    );
}

#[test]
fn test_interpolate_env_vars() {
    let _lock = acquire_env_lock();
    std::env::set_var("TEST_DRIVE_A", "/Volumes/TestDriveA");
    std::env::remove_var("TEST_UNSET_VAR");

    let template = "mount: ${TEST_DRIVE_A}\nbackup: ${TEST_UNSET_VAR:-/Volumes/DefaultBackup}\nempty: ${TEST_UNSET_VAR}";
    let result = DrivesConfigLoader::interpolate_env_vars(template);

    assert!(result.contains("mount: /Volumes/TestDriveA"));
    assert!(result.contains("backup: /Volumes/DefaultBackup"));
    assert!(result.contains("empty: "));

    std::env::remove_var("TEST_DRIVE_A");
}

#[test]
fn test_load_valid_drives_config() {
    let config = DrivesConfigLoader::load_from_str(SAMPLE_YAML, Some("yaml"))
        .expect("Config should parse cleanly");

    assert_eq!(config.version, "1.0");
    assert_eq!(config.groups.len(), 2);
    assert_eq!(config.groups["primary"].priority, 100);
    assert_eq!(config.groups["replica1"].priority, 80);
    assert_eq!(config.drives.len(), 2);
    assert_eq!(config.drives[0].id, "primary_main");
    assert_eq!(config.drives[0].group, "primary");
}

#[test]
fn test_reject_duplicate_drive_id() {
    let bad_yaml = r#"
version: "1.0"
groups:
  primary: { priority: 100 }
drives:
  - id: "drive_1"
    group: "primary"
    mount_path: "/Volumes/D1"
    master_path: "/Volumes/D1/master"
  - id: "drive_1"
    group: "primary"
    mount_path: "/Volumes/D2"
    master_path: "/Volumes/D2/master"
"#;

    let res = DrivesConfigLoader::load_from_str(bad_yaml, Some("yaml"));
    assert!(res.is_err(), "Duplicate drive id must be rejected");
}

#[test]
fn test_reject_unknown_group() {
    let bad_yaml = r#"
version: "1.0"
groups:
  primary: { priority: 100 }
drives:
  - id: "drive_1"
    group: "non_existent_group"
    mount_path: "/Volumes/D1"
    master_path: "/Volumes/D1/master"
"#;

    let res = DrivesConfigLoader::load_from_str(bad_yaml, Some("yaml"));
    assert!(res.is_err(), "Unknown group must be rejected");
}

#[test]
fn test_canonical_path_matcher_evaluation() {
    let config = DrivesConfigLoader::load_from_str(SAMPLE_YAML, Some("yaml")).unwrap();
    let matcher = CanonicalPathMatcher::new(config);

    // 1. Valid Primary Master Photo
    let p1 = Path::new("/Volumes/SP-1/SPSA/media/master/2021/30-Nov-2021/Photos/IMG_1042.JPG");
    match matcher.evaluate_path(p1) {
        CanonicalPathStatus::InMaster {
            year,
            date_folder,
            category_folder,
        } => {
            assert_eq!(year, 2021);
            assert_eq!(date_folder, "30-Nov-2021");
            assert_eq!(category_folder, "Photos");
        }
        other => panic!("Expected InMaster, got {:?}", other),
    }
    assert!(matcher.is_primary_master(p1));
    assert_eq!(matcher.get_path_priority(p1), 100);

    // 2. Valid Replica Master Video
    let p2 = Path::new("/Volumes/SP-2/SPSA/media/master/2021/30-Nov-2021/Videos/VID_0001.MOV");
    match matcher.evaluate_path(p2) {
        CanonicalPathStatus::InMaster {
            year,
            date_folder,
            category_folder,
        } => {
            assert_eq!(year, 2021);
            assert_eq!(date_folder, "30-Nov-2021");
            assert_eq!(category_folder, "Videos");
        }
        other => panic!("Expected InMaster, got {:?}", other),
    }
    assert!(
        !matcher.is_primary_master(p2),
        "Replica cannot be primary master"
    );
    assert_eq!(matcher.get_path_priority(p2), 80);

    // 3. Valid PhotosRaw category
    let p3 = Path::new("/Volumes/SP-1/SPSA/media/master/2021/30-Nov-2021/PhotosRaw/IMG_1042.CR3");
    assert!(matches!(
        matcher.evaluate_path(p3),
        CanonicalPathStatus::InMaster { .. }
    ));

    // 4. Outside Master Path (Unorganized / Scratch folder on Primary)
    let p4 = Path::new("/Volumes/SP-1/Downloads/OldCameraBackup/IMG_1042.JPG");
    assert_eq!(
        matcher.evaluate_path(p4),
        CanonicalPathStatus::OutsideMaster
    );
    assert!(!matcher.is_primary_master(p4));
    assert_eq!(matcher.get_path_priority(p4), 100);

    // 5. Unknown Drive (Not configured in drives.yaml)
    let p5 = Path::new("/Volumes/Unknown-Disk/IMG_1042.JPG");
    assert_eq!(matcher.evaluate_path(p5), CanonicalPathStatus::UnknownDrive);
    assert!(!matcher.is_primary_master(p5));
    assert_eq!(matcher.get_path_priority(p5), 0);

    // 6. Malformed Date in Master Path
    let p6 = Path::new("/Volumes/SP-1/SPSA/media/master/2021/bad_date/Photos/IMG_1042.JPG");
    assert_eq!(
        matcher.evaluate_path(p6),
        CanonicalPathStatus::OutsideMaster
    );

    // 7. Invalid Category in Master Path
    let p7 = Path::new("/Volumes/SP-1/SPSA/media/master/2021/30-Nov-2021/Documents/text.pdf");
    assert_eq!(
        matcher.evaluate_path(p7),
        CanonicalPathStatus::OutsideMaster
    );
}
