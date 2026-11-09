use criterion::{black_box, criterion_group, criterion_main, Criterion};
use metadata_extractor::application::MetadataExtractionEngine;
use metadata_extractor::domain::metadata_models::{TemporalMetadata, TimestampSource};
use metadata_extractor::domain::models::{DiscoveredMediaItem, MediaCategory};
use metadata_extractor::domain::ports::MediaMetadataExtractor;
use metadata_extractor::infrastructure::exif_parser::ExifMetadataExtractor;
use metadata_extractor::infrastructure::video_parser::VideoMetadataExtractor;
use std::path::PathBuf;

fn bench_temporal_date_parsing(c: &mut Criterion) {
    let raw_date = "2026:10:14 08:12:45";

    c.bench_function("parse_exif_date_standard", |b| {
        b.iter(|| {
            black_box(TemporalMetadata::parse_exif_date(
                black_box(raw_date),
                TimestampSource::ExifDateTimeOriginal,
            ));
        });
    });
}

fn bench_corrupt_exif_extraction(c: &mut Criterion) {
    let extractor = ExifMetadataExtractor::new();
    let corrupt_path = PathBuf::from("tests/fixtures/corrupt_samples/corrupt_exif.jpg");
    let item = DiscoveredMediaItem {
        path: corrupt_path,
        file_size_bytes: 22,
        modified_at: None,
        category: MediaCategory::Photo,
        extension: "jpg".to_string(),
    };

    c.bench_function("extract_corrupt_exif_file", |b| {
        b.iter(|| {
            let _ = black_box(extractor.extract_file(black_box(&item), None));
        });
    });
}

fn bench_valid_photo_throughput(c: &mut Criterion) {
    let engine = MetadataExtractionEngine::new();
    let valid_photo = PathBuf::from("tests/fixtures/valid_samples/valid_exif.jpg");
    let item = DiscoveredMediaItem {
        path: valid_photo,
        file_size_bytes: 1260,
        modified_at: None,
        category: MediaCategory::Photo,
        extension: "jpg".to_string(),
    };

    c.bench_function("extract_valid_photo_header_throughput", |b| {
        b.iter(|| {
            let _ = black_box(engine.extract_file(black_box(&item), None));
        });
    });
}

fn bench_video_container_throughput(c: &mut Criterion) {
    let extractor = VideoMetadataExtractor::new();
    let valid_video = PathBuf::from("tests/fixtures/valid_samples/valid_video.mp4");
    let item = DiscoveredMediaItem {
        path: valid_video,
        file_size_bytes: 512,
        modified_at: None,
        category: MediaCategory::Video,
        extension: "mp4".to_string(),
    };

    c.bench_function("extract_video_container_header_throughput", |b| {
        b.iter(|| {
            let _ = black_box(extractor.extract_file(black_box(&item), None));
        });
    });
}

criterion_group!(
    benches,
    bench_temporal_date_parsing,
    bench_corrupt_exif_extraction,
    bench_valid_photo_throughput,
    bench_video_container_throughput
);
criterion_main!(benches);
