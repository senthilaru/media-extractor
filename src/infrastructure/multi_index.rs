use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::domain::ports::MultiIndexLookup;

/// Multi-Index Hashing table implementing the Pigeonhole principle for 64-bit hashes.
/// Divides 64 bits into 4 x 16-bit blocks. If Hamming distance <= d, then at least one block
/// has Hamming distance <= floor(d / 4).
#[derive(Debug, Clone, Default)]
pub struct MultiIndexLookupTable {
    /// 4 index tables mapping 16-bit block value to list of (item id, full 64-bit hash)
    tables: [HashMap<u16, Vec<(Uuid, u64)>>; 4],
    /// All entries for fallback linear search if requested threshold is unusually large (> 11)
    all_entries: Vec<(Uuid, u64)>,
}

impl MultiIndexLookupTable {
    pub fn new() -> Self {
        Self {
            tables: [
                HashMap::new(),
                HashMap::new(),
                HashMap::new(),
                HashMap::new(),
            ],
            all_entries: Vec::new(),
        }
    }

    /// Decomposes a 64-bit integer into 4 x 16-bit blocks.
    #[inline]
    fn extract_chunks(hash: u64) -> [u16; 4] {
        [
            (hash >> 48) as u16,
            (hash >> 32) as u16,
            (hash >> 16) as u16,
            hash as u16,
        ]
    }
}

impl MultiIndexLookup for MultiIndexLookupTable {
    fn insert(&mut self, id: Uuid, hash: u64) {
        let chunks = Self::extract_chunks(hash);
        for (i, &chunk) in chunks.iter().enumerate() {
            self.tables[i].entry(chunk).or_default().push((id, hash));
        }
        self.all_entries.push((id, hash));
    }

    fn query_candidates(&self, hash: u64, max_distance: u32) -> Vec<(Uuid, u32)> {
        let max_chunk_dist = max_distance / 4;

        if max_chunk_dist > 2 {
            // Threshold is too large for 16-bit block perturbation search; fallback to direct scan
            let mut results = Vec::new();
            for &(id, cand_hash) in &self.all_entries {
                let dist = (hash ^ cand_hash).count_ones();
                if dist <= max_distance {
                    results.push((id, dist));
                }
            }
            return results;
        }

        let chunks = Self::extract_chunks(hash);
        let mut seen = HashSet::new();
        let mut candidates = Vec::new();

        for (i, &base_chunk) in chunks.iter().enumerate() {
            // 0-bit perturbation (exact match)
            if let Some(list) = self.tables[i].get(&base_chunk) {
                for &(cand_id, cand_hash) in list {
                    if seen.insert(cand_id) {
                        let dist = (hash ^ cand_hash).count_ones();
                        if dist <= max_distance {
                            candidates.push((cand_id, dist));
                        }
                    }
                }
            }

            // 1-bit perturbations
            if max_chunk_dist >= 1 {
                for bit1 in 0..16 {
                    let perturbed = base_chunk ^ (1u16 << bit1);
                    if let Some(list) = self.tables[i].get(&perturbed) {
                        for &(cand_id, cand_hash) in list {
                            if seen.insert(cand_id) {
                                let dist = (hash ^ cand_hash).count_ones();
                                if dist <= max_distance {
                                    candidates.push((cand_id, dist));
                                }
                            }
                        }
                    }
                }
            }

            // 2-bit perturbations
            if max_chunk_dist >= 2 {
                for bit1 in 0..15 {
                    for bit2 in (bit1 + 1)..16 {
                        let perturbed = base_chunk ^ (1u16 << bit1) ^ (1u16 << bit2);
                        if let Some(list) = self.tables[i].get(&perturbed) {
                            for &(cand_id, cand_hash) in list {
                                if seen.insert(cand_id) {
                                    let dist = (hash ^ cand_hash).count_ones();
                                    if dist <= max_distance {
                                        candidates.push((cand_id, dist));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        candidates
    }
}
