use metadata_extractor::domain::ports::MultiIndexLookup;
use metadata_extractor::infrastructure::multi_index::MultiIndexLookupTable;
use uuid::Uuid;

#[test]
fn test_multi_index_empty_table() {
    let table = MultiIndexLookupTable::new();
    let candidates = table.query_candidates(0x1234_5678_9ABC_DEF0, 10);
    assert!(candidates.is_empty());
}

#[test]
fn test_multi_index_exact_match() {
    let mut table = MultiIndexLookupTable::new();
    let id1 = Uuid::new_v4();
    let hash1 = 0xAAAA_BBBB_CCCC_DDDD;

    table.insert(id1, hash1);

    let candidates = table.query_candidates(hash1, 0);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].0, id1);
    assert_eq!(candidates[0].1, 0);
}

#[test]
fn test_multi_index_near_duplicate_lookup() {
    let mut table = MultiIndexLookupTable::new();
    let id1 = Uuid::new_v4();
    let id2 = Uuid::new_v4();
    let id3 = Uuid::new_v4();

    let base_hash = 0x1111_2222_3333_4444;
    // 3 bit flips
    let near_hash1 = base_hash ^ 0x0001_0001_0001_0000;
    // 8 bit flips (<= 10)
    let near_hash2 = base_hash ^ 0x0003_0003_0003_0003;
    // 20 bit flips (> 10)
    let far_hash = base_hash ^ 0x00FF_00FF_000F_0000;

    table.insert(id1, near_hash1);
    table.insert(id2, near_hash2);
    table.insert(id3, far_hash);

    let candidates = table.query_candidates(base_hash, 10);
    let ids: Vec<Uuid> = candidates.iter().map(|(id, _)| *id).collect();

    assert!(
        ids.contains(&id1),
        "Should contain near_hash1 (3 bit flips)"
    );
    assert!(
        ids.contains(&id2),
        "Should contain near_hash2 (8 bit flips)"
    );
    assert!(
        !ids.contains(&id3),
        "Should NOT contain far_hash (20 bit flips)"
    );

    // Check reported distances
    for (id, dist) in &candidates {
        if *id == id1 {
            assert_eq!(*dist, 3);
        } else if *id == id2 {
            assert_eq!(*dist, 8);
        }
    }
}

#[test]
fn test_multi_index_pigeonhole_boundary() {
    let mut table = MultiIndexLookupTable::new();
    let id = Uuid::new_v4();
    let base_hash: u64 = 0xF0F0_A5A5_5A5A_0F0F;
    let flipped_hash: u64 = base_hash ^ (0x3u64 << 48) ^ (0x3u64 << 32) ^ (0x7u64 << 16) ^ 0x7u64;
    assert_eq!((base_hash ^ flipped_hash).count_ones(), 10);

    table.insert(id, flipped_hash);

    let candidates = table.query_candidates(base_hash, 10);
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].0, id);
    assert_eq!(candidates[0].1, 10);

    // If max_distance is 9, this 10-bit difference should not be matched
    let candidates_9 = table.query_candidates(base_hash, 9);
    assert!(candidates_9.is_empty());
}
