use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tempfile::tempdir;

const BIN_PATH: &str = env!("CARGO_BIN_EXE_metadata-extractor");

#[test]
fn test_cli_help_when_no_arguments_provided() {
    let output = Command::new(BIN_PATH)
        .output()
        .expect("Failed to execute metadata-extractor binary");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Usage:"));
    assert!(stdout.contains("metadata-extractor"));
    assert!(stdout.contains("[PATH]"));
}

#[test]
fn test_unified_single_file_extraction_stdout() {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/valid_samples/valid_exif.jpg");
    assert!(fixture_path.exists(), "Fixture valid_exif.jpg must exist");

    let output = Command::new(BIN_PATH)
        .arg(&fixture_path)
        .output()
        .expect("Failed to execute single-file extraction");

    assert!(
        output.status.success(),
        "Extraction should succeed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let val: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("Stdout should be valid JSON: {}. Stdout: {}", e, stdout));

    // Verify file_context
    assert_eq!(val["file_context"]["extension"], "jpg");
    assert_eq!(val["file_context"]["media_category"], "Photo");
    assert!(val["file_context"]["file_size_bytes"].as_u64().unwrap() > 0);

    // Verify temporal integer facets
    assert!(val["temporal"]["year"].as_i64().is_some());
    assert!((1..=12).contains(&val["temporal"]["month"].as_i64().unwrap()));
    assert!((1..=31).contains(&val["temporal"]["day"].as_i64().unwrap()));

    // Verify fingerprint (US2)
    assert!(
        val["fingerprint"].is_object(),
        "Fingerprint must be present"
    );
    let fp = &val["fingerprint"];
    let crypto_hash = fp["cryptographic_hash"].as_str().unwrap();
    assert_eq!(crypto_hash.len(), 64, "BLAKE3 hash should be 64 hex chars");
    let sample_digest = fp["sample_digest"].as_str().unwrap();
    assert_eq!(
        sample_digest.len(),
        64,
        "BLAKE3 sample digest should be 64 hex chars"
    );

    assert!(
        fp["phash"].as_u64().is_some(),
        "pHash should be computed for photo"
    );
    let phash_hex = fp["phash_hex"].as_str().unwrap();
    assert_eq!(phash_hex.len(), 16, "pHash hex should be 16 chars");
    assert!(
        fp["rotated_hashes"].is_object(),
        "Rotated hashes should be present"
    );
    assert!(fp["rotated_hashes"]["hash_90"].is_number());
    assert!(fp["rotated_hashes"]["hash_180"].is_number());
    assert!(fp["rotated_hashes"]["hash_270"].is_number());
    assert!(fp["is_low_entropy"].is_boolean());
}

#[test]
fn test_unified_single_file_extraction_to_file() {
    let fixture_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/valid_samples/valid_video.mp4");
    assert!(fixture_path.exists(), "Fixture valid_video.mp4 must exist");

    let temp_dir = tempdir().expect("Failed to create tempdir");
    let out_file = temp_dir.path().join("video_meta.json");

    let output = Command::new(BIN_PATH)
        .arg(&fixture_path)
        .arg("-o")
        .arg(&out_file)
        .output()
        .expect("Failed to execute single-file extraction with output file");

    assert!(
        output.status.success(),
        "Extraction should succeed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Stdout should be empty because output went to file
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().is_empty(),
        "Stdout should be empty when writing to file"
    );

    // Read the output file
    let file_content =
        fs::read_to_string(&out_file).expect("Output file must exist and be readable");
    let val: serde_json::Value = serde_json::from_str(&file_content).unwrap_or_else(|e| {
        panic!(
            "Output file should be valid JSON: {}. Content: {}",
            e, file_content
        )
    });

    assert_eq!(val["file_context"]["extension"], "mp4");
    assert_eq!(val["file_context"]["media_category"], "Video");
    assert!(val["fingerprint"]["cryptographic_hash"].as_str().is_some());
    assert!(val["fingerprint"]["sample_digest"].as_str().is_some());
}

#[test]
fn test_unified_recursive_directory_scan_stdout() {
    let fixture_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/valid_samples");
    assert!(fixture_dir.exists(), "Fixture directory must exist");

    let output = Command::new(BIN_PATH)
        .arg(&fixture_dir)
        .arg("-q") // quiet mode to suppress stderr heartbeat
        .output()
        .expect("Failed to execute directory scan");

    assert!(
        output.status.success(),
        "Directory scan should succeed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();

    // valid_samples contains 5 files: valid_exif.jpg, valid_raw.tif, valid_video.mp4, rotated_video.mp4, tail_moov_video.mp4
    assert!(
        lines.len() >= 5,
        "Expected at least 5 media items, got {}",
        lines.len()
    );

    for line in &lines {
        let val: serde_json::Value = serde_json::from_str(line)
            .unwrap_or_else(|e| panic!("Line should be valid NDJSON: {}. Line: {}", e, line));

        assert!(val["id"].as_str().is_some(), "Document must have UUID id");
        assert!(
            val["file_context"].is_object(),
            "file_context must be present"
        );
        assert!(val["temporal"].is_object(), "temporal must be present");
        assert!(
            val["extraction_status"].is_object(),
            "extraction_status must be present"
        );

        // Verify fingerprint is present on 100% of discovered media items
        let fp = &val["fingerprint"];
        assert!(fp.is_object(), "100% of items must have fingerprint object");
        assert!(fp["cryptographic_hash"].as_str().is_some());
        assert_eq!(fp["cryptographic_hash"].as_str().unwrap().len(), 64);
        assert!(fp["sample_digest"].as_str().is_some());
        assert_eq!(fp["sample_digest"].as_str().unwrap().len(), 64);

        let category = val["file_context"]["media_category"].as_str().unwrap();
        if category == "Photo" || category == "RawPhoto" {
            // Check photo-specific perceptual hashes
            if fp["phash"].is_number() {
                assert_eq!(fp["phash_hex"].as_str().unwrap().len(), 16);
                assert!(fp["rotated_hashes"].is_object());
                assert!(fp["rotated_hashes"]["hash_90"].is_number());
            }
        }
    }
}

#[test]
fn test_unified_recursive_directory_scan_to_output_file() {
    let fixture_dir =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/valid_samples");
    assert!(fixture_dir.exists(), "Fixture directory must exist");

    let temp_dir = tempdir().expect("Failed to create tempdir");
    let out_file = temp_dir.path().join("catalog.ndjson");

    let output = Command::new(BIN_PATH)
        .arg(&fixture_dir)
        .arg("-o")
        .arg(&out_file)
        .output()
        .expect("Failed to execute directory scan with -o flag");

    assert!(
        output.status.success(),
        "Directory scan should succeed: {:?}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify stdout is clean
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.trim().is_empty(),
        "Stdout should be empty when writing to output file"
    );

    // Verify output file exists and contains valid NDJSON
    assert!(out_file.exists(), "Output catalog file must exist");
    let content = fs::read_to_string(&out_file).expect("Failed to read output file");
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    assert!(
        lines.len() >= 5,
        "Expected at least 5 lines in catalog file, got {}",
        lines.len()
    );

    for line in &lines {
        let val: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|e| {
            panic!("Line in file should be valid NDJSON: {}. Line: {}", e, line)
        });
        assert!(val["id"].as_str().is_some());
        assert!(val["fingerprint"]["cryptographic_hash"].as_str().is_some());
    }
}

#[test]
fn test_unified_scan_low_entropy_and_nested_directories() {
    // Test nested subdirectories and blank frame detection
    let temp_dir = tempdir().expect("Failed to create tempdir");
    let root = temp_dir.path();
    let sub1 = root.join("2026/10");
    let sub2 = root.join("2026/11");
    fs::create_dir_all(&sub1).unwrap();
    fs::create_dir_all(&sub2).unwrap();

    let dedup_samples =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/dedup_samples");
    assert!(dedup_samples.exists());

    // Copy a blank black image and a distinct photo into nested directories
    fs::copy(
        dedup_samples.join("blank_black.jpg"),
        sub1.join("black.jpg"),
    )
    .unwrap();
    fs::copy(
        dedup_samples.join("photo_original.jpg"),
        sub2.join("original.jpg"),
    )
    .unwrap();

    let output = Command::new(BIN_PATH)
        .arg(root)
        .arg("-q")
        .output()
        .expect("Failed to execute recursive scan on nested tree");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();
    assert_eq!(lines.len(), 2, "Should discover both nested files");

    let docs: Vec<serde_json::Value> = lines
        .into_iter()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();

    let black_doc = docs
        .iter()
        .find(|d| d["file_context"]["file_name"] == "black.jpg")
        .expect("black.jpg must be found");
    assert_eq!(
        black_doc["fingerprint"]["is_low_entropy"].as_bool(),
        Some(true),
        "blank_black.jpg should be identified as low entropy"
    );

    let orig_doc = docs
        .iter()
        .find(|d| d["file_context"]["file_name"] == "original.jpg")
        .expect("original.jpg must be found");
    assert_eq!(
        orig_doc["fingerprint"]["is_low_entropy"].as_bool(),
        Some(false),
        "photo_original.jpg should NOT be identified as low entropy"
    );
}
