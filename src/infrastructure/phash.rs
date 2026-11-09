use std::f64::consts::PI;

use crate::domain::dedup_models::RotatedHashes;

/// Pure-Rust Discrete Cosine Transform (DCT) 64-bit perceptual visual hasher.
#[derive(Debug, Default, Clone)]
pub struct DctPerceptualHasher;

impl DctPerceptualHasher {
    pub fn new() -> Self {
        Self
    }

    /// Computes variance across the 1024 pixels. Returns true if variance < 5.0 (monochromatic/blank).
    pub fn is_low_entropy(matrix: &[u8; 1024]) -> bool {
        let sum: f64 = matrix.iter().map(|&p| p as f64).sum();
        let mean = sum / 1024.0;
        let var: f64 = matrix
            .iter()
            .map(|&p| {
                let diff = p as f64 - mean;
                diff * diff
            })
            .sum::<f64>()
            / 1024.0;

        var < 5.0
    }

    /// Rotates a 32x32 raster 90 degrees clockwise.
    pub fn rotate_90(src: &[u8; 1024]) -> [u8; 1024] {
        let mut dst = [0u8; 1024];
        for r in 0..32 {
            for c in 0..32 {
                let dst_r = c;
                let dst_c = 31 - r;
                dst[dst_r * 32 + dst_c] = src[r * 32 + c];
            }
        }
        dst
    }

    /// Rotates a 32x32 raster 180 degrees.
    pub fn rotate_180(src: &[u8; 1024]) -> [u8; 1024] {
        let mut dst = [0u8; 1024];
        for i in 0..1024 {
            dst[1023 - i] = src[i];
        }
        dst
    }

    /// Rotates a 32x32 raster 270 degrees clockwise.
    pub fn rotate_270(src: &[u8; 1024]) -> [u8; 1024] {
        let mut dst = [0u8; 1024];
        for r in 0..32 {
            for c in 0..32 {
                let dst_r = 31 - c;
                let dst_c = r;
                dst[dst_r * 32 + dst_c] = src[r * 32 + c];
            }
        }
        dst
    }

    /// Computes the 64-bit DCT perceptual hash from a 32x32 grayscale matrix.
    pub fn compute_hash(matrix: &[u8; 1024]) -> u64 {
        // Step 1: Compute 1D DCT on rows, extracting first 8 frequencies
        let mut row_dct = [0.0f64; 32 * 8];
        for r in 0..32 {
            for u in 0..8 {
                let mut sum = 0.0f64;
                for c in 0..32 {
                    let val = matrix[r * 32 + c] as f64;
                    let angle = ((2 * c + 1) as f64 * u as f64 * PI) / 64.0;
                    sum += val * angle.cos();
                }
                row_dct[r * 8 + u] = sum;
            }
        }

        // Step 2: Compute 1D DCT on columns, extracting the 8x8 low-frequency matrix
        let mut dct_8x8 = [0.0f64; 64];
        for v in 0..8 {
            for u in 0..8 {
                let mut sum = 0.0f64;
                for r in 0..32 {
                    let val = row_dct[r * 8 + u];
                    let angle = ((2 * r + 1) as f64 * v as f64 * PI) / 64.0;
                    sum += val * angle.cos();
                }
                dct_8x8[v * 8 + u] = sum;
            }
        }

        // Step 3: Compute median of the 64 coefficients (excluding DC component at index 0)
        let mut ac_coeffs: Vec<f64> = dct_8x8[1..64].to_vec();
        ac_coeffs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = if ac_coeffs.len().is_multiple_of(2) {
            let mid = ac_coeffs.len() / 2;
            (ac_coeffs[mid - 1] + ac_coeffs[mid]) / 2.0
        } else {
            ac_coeffs[ac_coeffs.len() / 2]
        };

        // Step 4: Construct 64-bit integer
        let mut hash = 0u64;
        for (i, &coeff) in dct_8x8.iter().enumerate() {
            if coeff > median {
                hash |= 1u64 << (63 - i);
            }
        }

        hash
    }

    /// Computes the canonical 0° hash and all 3 orthogonal rotation hashes.
    pub fn compute_all_hashes(matrix: &[u8; 1024]) -> (u64, RotatedHashes, bool) {
        let is_low_entropy = Self::is_low_entropy(matrix);
        let hash_0 = Self::compute_hash(matrix);

        let m90 = Self::rotate_90(matrix);
        let hash_90 = Self::compute_hash(&m90);

        let m180 = Self::rotate_180(matrix);
        let hash_180 = Self::compute_hash(&m180);

        let m270 = Self::rotate_270(matrix);
        let hash_270 = Self::compute_hash(&m270);

        let rotated = RotatedHashes {
            hash_90,
            hash_180,
            hash_270,
        };

        (hash_0, rotated, is_low_entropy)
    }

    /// Computes Hamming distance (bit difference) between two 64-bit hashes.
    pub fn hamming_distance(h1: u64, h2: u64) -> u32 {
        (h1 ^ h2).count_ones()
    }
}
