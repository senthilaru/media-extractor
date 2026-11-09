use metadata_extractor::domain::models::MediaCategory;
use metadata_extractor::domain::ports::MediaClassifier;
use metadata_extractor::infrastructure::classifier::PhfMediaClassifier;
use std::path::Path;

#[test]
fn test_classify_photos() {
    let classifier = PhfMediaClassifier::new();

    assert_eq!(
        classifier.classify(Path::new("photo.jpg")),
        Some(MediaCategory::Photo)
    );
    assert_eq!(
        classifier.classify(Path::new("folder/IMG_001.JPEG")),
        Some(MediaCategory::Photo)
    );
    assert_eq!(
        classifier.classify(Path::new("screenshot.PNG")),
        Some(MediaCategory::Photo)
    );
    assert_eq!(
        classifier.classify(Path::new("image.heic")),
        Some(MediaCategory::Photo)
    );
    assert_eq!(
        classifier.classify(Path::new("image.WEBP")),
        Some(MediaCategory::Photo)
    );
}

#[test]
fn test_classify_videos() {
    let classifier = PhfMediaClassifier::new();

    assert_eq!(
        classifier.classify(Path::new("clip.mp4")),
        Some(MediaCategory::Video)
    );
    assert_eq!(
        classifier.classify(Path::new("movie.MOV")),
        Some(MediaCategory::Video)
    );
    assert_eq!(
        classifier.classify(Path::new("recording.mkv")),
        Some(MediaCategory::Video)
    );
    assert_eq!(
        classifier.classify(Path::new("stream.webm")),
        Some(MediaCategory::Video)
    );
}

#[test]
fn test_classify_raw_photos() {
    let classifier = PhfMediaClassifier::new();

    assert_eq!(
        classifier.classify(Path::new("canon.CR3")),
        Some(MediaCategory::RawPhoto)
    );
    assert_eq!(
        classifier.classify(Path::new("canon_old.cr2")),
        Some(MediaCategory::RawPhoto)
    );
    assert_eq!(
        classifier.classify(Path::new("nikon.NEF")),
        Some(MediaCategory::RawPhoto)
    );
    assert_eq!(
        classifier.classify(Path::new("sony.arw")),
        Some(MediaCategory::RawPhoto)
    );
    assert_eq!(
        classifier.classify(Path::new("adobe.DNG")),
        Some(MediaCategory::RawPhoto)
    );
}

#[test]
fn test_classify_non_media() {
    let classifier = PhfMediaClassifier::new();

    assert_eq!(classifier.classify(Path::new("document.pdf")), None);
    assert_eq!(classifier.classify(Path::new("notes.txt")), None);
    assert_eq!(classifier.classify(Path::new(".DS_Store")), None);
    assert_eq!(classifier.classify(Path::new("archive.zip")), None);
    assert_eq!(classifier.classify(Path::new("binary")), None);
    assert_eq!(classifier.classify(Path::new("script.rs")), None);
}

#[test]
fn test_is_supported_extension() {
    let classifier = PhfMediaClassifier::new();

    assert!(classifier.is_supported_extension("jpg"));
    assert!(classifier.is_supported_extension("JPG"));
    assert!(classifier.is_supported_extension("mp4"));
    assert!(classifier.is_supported_extension("cr3"));
    assert!(!classifier.is_supported_extension("txt"));
    assert!(!classifier.is_supported_extension("exe"));
}
