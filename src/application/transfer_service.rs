use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::domain::dedup_models::PathResolution;
use crate::domain::errors::DeduplicationError;
use crate::domain::models::MediaCategory;
use crate::domain::ports::MediaFingerprinter;
use crate::infrastructure::blake3_hasher::Blake3MediaHasher;

/// Shared service handling verified streaming file transfers, collision resolution,
/// and canonical path calculations.
#[derive(Debug, Default, Clone)]
pub struct UnifiedFileTransferService;

impl UnifiedFileTransferService {
    pub fn new() -> Self {
        Self
    }

    /// Formats calendar components into `<Year>` and `<DD-Mon-YYYY>` folder name.
    pub fn format_date_folder(year: i32, month: u32, day: u32) -> (i32, String) {
        let months = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let m_idx = (month.saturating_sub(1) as usize).min(11);
        let month_str = months[m_idx];
        let date_folder = format!("{:02}-{}-{}", day, month_str, year);
        (year, date_folder)
    }

    /// Determines the canonical category folder name.
    pub fn category_folder_name(cat: MediaCategory) -> &'static str {
        match cat {
            MediaCategory::Photo => "Photos",
            MediaCategory::RawPhoto => "PhotosRaw",
            MediaCategory::Video => "Videos",
        }
    }

    /// Builds a relative canonical path: `<Year>/<DD-Mon-YYYY>/{Videos,Photos,PhotosRaw}/<Filename>`
    pub fn build_canonical_relative_path(
        year: i32,
        month: u32,
        day: u32,
        category: MediaCategory,
        filename: &str,
    ) -> PathBuf {
        let (y, date_folder) = Self::format_date_folder(year, month, day);
        let cat_folder = Self::category_folder_name(category);
        PathBuf::from(format!("{}", y))
            .join(date_folder)
            .join(cat_folder)
            .join(filename)
    }

    /// Evaluates destination collision and returns `Available`, `IdenticalExists`, or `CollisionResolved`.
    pub fn resolve_collision_path(
        target_path: &Path,
        source_size: u64,
        source_hash: Option<&str>,
        hasher: &dyn MediaFingerprinter,
    ) -> PathResolution {
        if !target_path.exists() {
            return PathResolution::Available(target_path.to_path_buf());
        }

        // Check if existing file is already identical in byte size and cryptographic hash
        let existing_len = std::fs::metadata(target_path).map(|m| m.len()).unwrap_or(0);

        if existing_len == source_size && source_hash.is_some() {
            if let Ok(existing_hash) = hasher.compute_cryptographic_hash(target_path) {
                if Some(existing_hash.as_str()) == source_hash {
                    return PathResolution::IdenticalExists(target_path.to_path_buf());
                }
            }
        }

        // Resolve collision by appending an incrementing suffix `_1`, `_2`, ...
        let parent = target_path.parent().unwrap_or_else(|| Path::new(""));
        let file_stem = target_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("file");
        let ext = target_path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e))
            .unwrap_or_default();

        let mut counter = 1usize;
        loop {
            let candidate_name = format!("{}_{}{}", file_stem, counter, ext);
            let candidate_path = parent.join(candidate_name);
            if !candidate_path.exists() {
                return PathResolution::CollisionResolved(candidate_path);
            }
            counter += 1;
        }
    }

    /// Copies a file using a bounded 1MB asynchronous streaming buffer (`tokio::io`),
    /// verifies byte size and BLAKE3 cryptographic digest, and purges the destination file
    /// if any verification fails.
    pub async fn stream_copy_verified(
        source: &Path,
        target: &Path,
        expected_size: u64,
        expected_hash: Option<&str>,
        verify_hash: bool,
    ) -> Result<u64, DeduplicationError> {
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                DeduplicationError::TransferFailed {
                    source_path: source.to_path_buf(),
                    target_path: target.to_path_buf(),
                    reason: format!(
                        "Failed creating target directory '{}': {}",
                        parent.display(),
                        e
                    ),
                }
            })?;
        }

        let mut src_file = tokio::fs::File::open(source).await.map_err(|e| {
            DeduplicationError::TransferFailed {
                source_path: source.to_path_buf(),
                target_path: target.to_path_buf(),
                reason: format!("Failed opening source file: {}", e),
            }
        })?;

        let mut dst_file = tokio::fs::File::create(target).await.map_err(|e| {
            DeduplicationError::TransferFailed {
                source_path: source.to_path_buf(),
                target_path: target.to_path_buf(),
                reason: format!("Failed creating target file: {}", e),
            }
        })?;

        let mut buffer = vec![0u8; 1024 * 1024]; // 1MB bounded buffer
        let mut total_copied = 0u64;

        loop {
            let bytes_read = src_file.read(&mut buffer).await.map_err(|e| {
                DeduplicationError::TransferFailed {
                    source_path: source.to_path_buf(),
                    target_path: target.to_path_buf(),
                    reason: format!("Read error during transfer: {}", e),
                }
            })?;

            if bytes_read == 0 {
                break;
            }

            dst_file
                .write_all(&buffer[..bytes_read])
                .await
                .map_err(|e| DeduplicationError::TransferFailed {
                    source_path: source.to_path_buf(),
                    target_path: target.to_path_buf(),
                    reason: format!("Write error during transfer: {}", e),
                })?;

            total_copied += bytes_read as u64;
        }

        dst_file
            .flush()
            .await
            .map_err(|e| DeduplicationError::TransferFailed {
                source_path: source.to_path_buf(),
                target_path: target.to_path_buf(),
                reason: format!("Flush error on target file: {}", e),
            })?;

        // 1. Verify byte size
        if total_copied != expected_size {
            let _ = tokio::fs::remove_file(target).await;
            return Err(DeduplicationError::TransferFailed {
                source_path: source.to_path_buf(),
                target_path: target.to_path_buf(),
                reason: format!(
                    "Size mismatch on '{}': expected {} bytes, copied {} bytes",
                    target.display(),
                    expected_size,
                    total_copied
                ),
            });
        }

        // 2. Verify BLAKE3 hash if requested
        if verify_hash {
            let hasher = Blake3MediaHasher::new();
            match hasher.compute_cryptographic_hash(target) {
                Ok(dest_hash) => {
                    if let Some(exp_hash) = expected_hash {
                        if !exp_hash.is_empty() && exp_hash != dest_hash {
                            let _ = tokio::fs::remove_file(target).await;
                            return Err(DeduplicationError::TransferFailed {
                                source_path: source.to_path_buf(),
                                target_path: target.to_path_buf(),
                                reason: format!(
                                    "Hash mismatch on '{}': expected {}, got {}",
                                    target.display(),
                                    exp_hash,
                                    dest_hash
                                ),
                            });
                        }
                    }
                }
                Err(e) => {
                    let _ = tokio::fs::remove_file(target).await;
                    return Err(DeduplicationError::TransferFailed {
                        source_path: source.to_path_buf(),
                        target_path: target.to_path_buf(),
                        reason: format!(
                            "Failed computing hash on destination file '{}': {}",
                            target.display(),
                            e
                        ),
                    });
                }
            }
        }

        Ok(total_copied)
    }
}
