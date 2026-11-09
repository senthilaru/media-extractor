use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use crate::domain::dedup_models::{FingerprintConfig, MediaFingerprint};
use crate::domain::errors::FingerprintError;
use crate::domain::metadata_models::MediaMetadataDocument;
use crate::domain::models::{DiscoveredMediaItem, MediaCategory};
use crate::domain::ports::MediaFingerprinter;
use crate::infrastructure::phash::DctPerceptualHasher;
use crate::infrastructure::raster_decoder::RasterDecoder;

/// Infrastructure adapter for BLAKE3 streaming cryptographic digests and fast sample hashing.
#[derive(Debug, Default, Clone)]
pub struct Blake3MediaHasher;

impl Blake3MediaHasher {
    pub fn new() -> Self {
        Self
    }
}

impl MediaFingerprinter for Blake3MediaHasher {
    fn fingerprint_file(
        &self,
        item: &DiscoveredMediaItem,
        metadata: Option<&MediaMetadataDocument>,
        _config: &FingerprintConfig,
    ) -> Result<MediaFingerprint, FingerprintError> {
        let _span = tracing::debug_span!("fingerprint_file", path = %item.path.display());
        let _enter = _span.enter();

        let path = &item.path;
        if !path.exists() {
            return Err(FingerprintError::FileNotFound(path.clone()));
        }

        let metadata_fs = std::fs::metadata(path).map_err(|e| FingerprintError::Io {
            path: path.clone(),
            source: e,
        })?;

        let file_size = metadata_fs.len();
        if file_size == 0 {
            return Err(FingerprintError::ZeroByteFile(path.clone()));
        }

        let sample_digest = Some(self.compute_sample_digest(path)?);

        let should_hash = _config.enable_cryptographic_hash
            && match _config.max_hash_size_bytes {
                Some(limit) => file_size <= limit,
                None => true,
            };

        let cryptographic_hash = if should_hash {
            self.compute_cryptographic_hash(path)?
        } else {
            // For files exceeding threshold or with full hash disabled, use fast sample digest
            sample_digest.clone().unwrap_or_default()
        };

        let dimensions = metadata.and_then(|m| m.dimensions);

        let mut perceptual_hash = None;
        let mut rotated_hashes = None;
        let mut is_low_entropy = false;

        if _config.enable_perceptual && item.category != MediaCategory::Video {
            if let Ok(matrix) = RasterDecoder::decode_thumbnail_32x32(path) {
                if _config.enable_rotation {
                    let (h0, rot, low_e) = DctPerceptualHasher::compute_all_hashes(&matrix);
                    perceptual_hash = Some(h0);
                    rotated_hashes = Some(rot);
                    is_low_entropy = low_e;
                } else {
                    perceptual_hash = Some(DctPerceptualHasher::compute_hash(&matrix));
                    is_low_entropy = DctPerceptualHasher::is_low_entropy(&matrix);
                }
            }
        }

        let phash_hex = perceptual_hash.map(|h| format!("{:016x}", h));

        Ok(MediaFingerprint {
            id: uuid::Uuid::new_v4(),
            file_path: path.clone(),
            file_size_bytes: file_size,
            media_category: item.category,
            dimensions,
            sample_digest,
            cryptographic_hash,
            perceptual_hash,
            phash_hex,
            rotated_hashes,
            is_low_entropy,
            fingerprinted_at: chrono::Utc::now(),
        })
    }

    fn compute_sample_digest(&self, path: &Path) -> Result<String, FingerprintError> {
        let file = File::open(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                FingerprintError::FileNotFound(path.to_path_buf())
            } else {
                FingerprintError::Io {
                    path: path.to_path_buf(),
                    source: e,
                }
            }
        })?;

        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if len == 0 {
            return Err(FingerprintError::ZeroByteFile(path.to_path_buf()));
        }

        let mut reader = BufReader::new(file);
        let mut hasher = blake3::Hasher::new();
        hasher.update(&len.to_le_bytes());

        const CHUNK_SIZE: usize = 16384; // 16 KB

        if len <= (CHUNK_SIZE * 2) as u64 {
            // File is small, read all of it
            let mut buf = Vec::with_capacity(len as usize);
            reader
                .read_to_end(&mut buf)
                .map_err(|e| FingerprintError::Io {
                    path: path.to_path_buf(),
                    source: e,
                })?;
            hasher.update(&buf);
        } else {
            // Sample header (first 16 KB)
            let mut head = vec![0u8; CHUNK_SIZE];
            reader
                .read_exact(&mut head)
                .map_err(|e| FingerprintError::Io {
                    path: path.to_path_buf(),
                    source: e,
                })?;
            hasher.update(&head);

            // Sample trailer (last 16 KB)
            reader
                .seek(SeekFrom::End(-(CHUNK_SIZE as i64)))
                .map_err(|e| FingerprintError::Io {
                    path: path.to_path_buf(),
                    source: e,
                })?;
            let mut tail = vec![0u8; CHUNK_SIZE];
            reader
                .read_exact(&mut tail)
                .map_err(|e| FingerprintError::Io {
                    path: path.to_path_buf(),
                    source: e,
                })?;
            hasher.update(&tail);
        }

        Ok(hasher.finalize().to_hex().to_string())
    }

    fn compute_cryptographic_hash(&self, path: &Path) -> Result<String, FingerprintError> {
        let mut hasher = blake3::Hasher::new();

        // Attempt memory-mapped multi-threaded hashing for maximum throughput
        if hasher.update_mmap_rayon(path).is_ok() {
            return Ok(hasher.finalize().to_hex().to_string());
        }

        // Fallback to buffered streaming if mmap fails (e.g. permission or special file)
        let file = File::open(path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                FingerprintError::FileNotFound(path.to_path_buf())
            } else {
                FingerprintError::Io {
                    path: path.to_path_buf(),
                    source: e,
                }
            }
        })?;

        let mut reader = BufReader::with_capacity(65536, file);
        let mut buffer = [0u8; 65536];

        loop {
            let bytes_read = reader.read(&mut buffer).map_err(|e| FingerprintError::Io {
                path: path.to_path_buf(),
                source: e,
            })?;

            if bytes_read == 0 {
                break;
            }

            hasher.update(&buffer[..bytes_read]);
        }

        Ok(hasher.finalize().to_hex().to_string())
    }

    fn compute_perceptual_hash(&self, path: &Path) -> Result<Option<u64>, FingerprintError> {
        let matrix = RasterDecoder::decode_thumbnail_32x32(path)?;
        let hash = DctPerceptualHasher::compute_hash(&matrix);
        Ok(Some(hash))
    }
}
