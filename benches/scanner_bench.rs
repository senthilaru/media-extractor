use criterion::{black_box, criterion_group, criterion_main, Criterion};
use metadata_extractor::domain::ports::MediaClassifier;
use metadata_extractor::infrastructure::classifier::PhfMediaClassifier;
use std::path::Path;

fn bench_classifier(c: &mut Criterion) {
    let classifier = PhfMediaClassifier::new();
    let paths = [
        Path::new("/photos/2026/vacation/IMG_1042.jpg"),
        Path::new("/photos/2026/vacation/IMG_1043.CR3"),
        Path::new("/photos/2026/vacation/VID_0012.mp4"),
        Path::new("/photos/2026/vacation/notes.txt"),
        Path::new("/photos/2026/vacation/screenshot.PNG"),
        Path::new("/photos/2026/vacation/raw_nikon.NEF"),
    ];

    c.bench_function("classifier_classify_batch", |b| {
        b.iter(|| {
            for path in &paths {
                black_box(classifier.classify(black_box(path)));
            }
        });
    });
}

criterion_group!(benches, bench_classifier);
criterion_main!(benches);
