use criterion::{black_box, criterion_group, criterion_main, Criterion};
use metadata_extractor::domain::dedup_models::FingerprintConfig;
use metadata_extractor::domain::models::{DiscoveredMediaItem, MediaCategory};
use metadata_extractor::domain::ports::{MediaFingerprinter, MultiIndexLookup};
use metadata_extractor::infrastructure::blake3_hasher::Blake3MediaHasher;
use metadata_extractor::infrastructure::multi_index::MultiIndexLookupTable;
use metadata_extractor::infrastructure::phash::DctPerceptualHasher;
use std::path::PathBuf;
use uuid::Uuid;

fn bench_dct_phash_computation(c: &mut Criterion) {
    let mut matrix = [0u8; 1024];
    for (i, item) in matrix.iter_mut().enumerate() {
        *item = (i % 256) as u8;
    }

    c.bench_function("dct_phash_computation_32x32", |b| {
        b.iter(|| {
            black_box(DctPerceptualHasher::compute_hash(black_box(&matrix)));
        });
    });
}

fn bench_blake3_cryptographic_hashing(c: &mut Criterion) {
    let hasher = Blake3MediaHasher::new();
    let photo_path = PathBuf::from("tests/fixtures/dedup_samples/photo_original.jpg");

    c.bench_function("blake3_cryptographic_hash_file", |b| {
        b.iter(|| {
            let _ = black_box(hasher.compute_cryptographic_hash(black_box(&photo_path)));
        });
    });
}

fn bench_multi_index_lookup(c: &mut Criterion) {
    let mut table = MultiIndexLookupTable::new();
    let query_hash = 0x1234_5678_9ABC_DEF0u64;

    // Seed table with 1,000 items
    for i in 0..1000u64 {
        table.insert(Uuid::new_v4(), query_hash ^ (i << 16));
    }

    c.bench_function("multi_index_lookup_query_candidates_d10", |b| {
        b.iter(|| {
            black_box(table.query_candidates(black_box(query_hash), black_box(10)));
        });
    });
}

fn bench_fingerprint_file_throughput(c: &mut Criterion) {
    let hasher = Blake3MediaHasher::new();
    let photo_path = PathBuf::from("tests/fixtures/dedup_samples/photo_original.jpg");
    let item = DiscoveredMediaItem {
        path: photo_path,
        file_size_bytes: 1443,
        modified_at: None,
        category: MediaCategory::Photo,
        extension: "jpg".to_string(),
    };
    let config = FingerprintConfig {
        enable_perceptual: true,
        enable_rotation: true,
    };

    c.bench_function("fingerprint_file_photo_phash_and_crypto", |b| {
        b.iter(|| {
            let _ = black_box(hasher.fingerprint_file(black_box(&item), None, black_box(&config)));
        });
    });
}

criterion_group!(
    benches,
    bench_dct_phash_computation,
    bench_blake3_cryptographic_hashing,
    bench_multi_index_lookup,
    bench_fingerprint_file_throughput
);
criterion_main!(benches);
