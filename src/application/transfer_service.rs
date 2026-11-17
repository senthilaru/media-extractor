use std::path::{Path, PathBuf};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::domain::dedup_models::{PathResolution, TransferBufferConfig};
use crate::domain::errors::DeduplicationError;
use crate::domain::models::MediaCategory;
use crate::domain::ports::MediaFingerprinter;
use crate::infrastructure::blake3_hasher::Blake3MediaHasher;

/// Incremental streaming hasher for in-flight verification during file transfer.
/// Computes both the full-file BLAKE3 hash and the Tier 2 sample digest in-flight,
/// allowing verification against either full cryptographic digests or sample digests
/// (used for large media files exceeding the full hashing threshold).
#[derive(Debug)]
pub struct InFlightStreamHasher {
    full_hasher: blake3::Hasher,
    expected_hash: Option<String>,
    expected_size: u64,
    small_buf: Vec<u8>,
    head_buf: Vec<u8>,
    tail_buf: Vec<u8>,
    total_bytes: u64,
}

impl InFlightStreamHasher {
    pub const SAMPLE_CHUNK: usize = 16384; // 16 KB

    /// Creates a new in-flight hasher with an optional expected hexadecimal hash and expected size.
    pub fn new(expected_hash: Option<&str>, expected_size: u64) -> Self {
        Self {
            full_hasher: blake3::Hasher::new(),
            expected_hash: expected_hash.map(|s| s.to_string()),
            expected_size,
            small_buf: if expected_size <= (Self::SAMPLE_CHUNK * 2) as u64 {
                Vec::with_capacity(expected_size as usize)
            } else {
                Vec::new()
            },
            head_buf: Vec::with_capacity(Self::SAMPLE_CHUNK),
            tail_buf: Vec::with_capacity(Self::SAMPLE_CHUNK),
            total_bytes: 0,
        }
    }

    /// Feeds a chunk of transferred bytes into the cryptographic hash accumulator.
    pub fn update(&mut self, chunk: &[u8]) {
        self.full_hasher.update(chunk);

        if self.expected_size <= (Self::SAMPLE_CHUNK * 2) as u64 {
            self.small_buf.extend_from_slice(chunk);
        } else {
            // Collect first 16 KB
            if self.head_buf.len() < Self::SAMPLE_CHUNK {
                let needed = Self::SAMPLE_CHUNK - self.head_buf.len();
                let take = needed.min(chunk.len());
                self.head_buf.extend_from_slice(&chunk[..take]);
            }

            // Track rolling last 16 KB
            if chunk.len() >= Self::SAMPLE_CHUNK {
                self.tail_buf.clear();
                self.tail_buf
                    .extend_from_slice(&chunk[chunk.len() - Self::SAMPLE_CHUNK..]);
            } else {
                self.tail_buf.extend_from_slice(chunk);
                if self.tail_buf.len() > Self::SAMPLE_CHUNK {
                    let overflow = self.tail_buf.len() - Self::SAMPLE_CHUNK;
                    self.tail_buf.drain(0..overflow);
                }
            }
        }

        self.total_bytes += chunk.len() as u64;
    }

    /// Computes the finalized 64-character hexadecimal digest of the entire stream.
    pub fn finalize_hex(&self) -> String {
        self.full_hasher.finalize().to_hex().to_string()
    }

    /// Computes the Tier 2 sample digest (first 16KB + last 16KB + size LE bytes).
    pub fn finalize_sample_digest_hex(&self) -> String {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.total_bytes.to_le_bytes());

        if self.total_bytes <= (Self::SAMPLE_CHUNK * 2) as u64 {
            hasher.update(&self.small_buf);
        } else {
            hasher.update(&self.head_buf);
            hasher.update(&self.tail_buf);
        }

        hasher.finalize().to_hex().to_string()
    }

    /// Verifies the in-flight hash against the expected hash.
    pub fn finalize_and_verify(&self) -> Result<String, DeduplicationError> {
        self.finalize_and_verify_with_paths(Path::new(""), Path::new(""))
    }

    /// Verifies the in-flight hash against the expected hash, reporting full path context on mismatch.
    /// Accepts either the full bitwise BLAKE3 hash or the Tier 2 sample digest.
    pub fn finalize_and_verify_with_paths(
        &self,
        source: &Path,
        target: &Path,
    ) -> Result<String, DeduplicationError> {
        let full_computed = self.finalize_hex();
        let sample_computed = self.finalize_sample_digest_hex();

        if let Some(ref expected) = self.expected_hash {
            if !expected.is_empty() && *expected != full_computed && *expected != sample_computed {
                return Err(DeduplicationError::TransferFailed {
                    source_path: source.to_path_buf(),
                    target_path: target.to_path_buf(),
                    reason: format!(
                        "In-flight hash mismatch on '{}': expected {}, computed {}",
                        target.display(),
                        expected,
                        full_computed
                    ),
                });
            }
        }
        Ok(full_computed)
    }
}

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
            MediaCategory::Photo | MediaCategory::Sidecar => "Photos",
            MediaCategory::RawPhoto => "PhotosRaw",
            MediaCategory::Video => "Videos",
        }
    }

    /// Checks if an extension is a known companion sidecar format.
    pub fn is_sidecar_extension(ext: &str) -> bool {
        ext.eq_ignore_ascii_case("aae")
            || ext.eq_ignore_ascii_case("ifo")
            || ext.eq_ignore_ascii_case("bup")
    }

    /// Checks if a media file stem and sidecar stem match, supporting:
    /// 1. Exact match (`m == s`)
    /// 2. Apple edit variants (`IMG_1234`, `IMG_E1234`, `IMG_O1234`, `OIMG_1234`)
    /// 3. DVD Title Set sets (`VTS_xx_y` matches `VTS_xx_0` or `VTS_xx_y`)
    /// 4. DVD Disc sets (`VIDEO_TS` matches `VIDEO_TS`)
    pub fn is_companion_sidecar_stem_match(
        media_stem: &str,
        sidecar_stem: &str,
        sidecar_ext: &str,
    ) -> bool {
        if media_stem.eq_ignore_ascii_case(sidecar_stem) {
            return true;
        }

        if sidecar_ext.eq_ignore_ascii_case("aae") {
            return Self::is_apple_sidecar_stem_match(media_stem, sidecar_stem);
        }

        if sidecar_ext.eq_ignore_ascii_case("ifo") || sidecar_ext.eq_ignore_ascii_case("bup") {
            let m = media_stem.to_ascii_lowercase();
            let s = sidecar_stem.to_ascii_lowercase();
            if m.starts_with("vts_") && s.starts_with("vts_") {
                let parts_m: Vec<&str> = m.split('_').collect();
                let parts_s: Vec<&str> = s.split('_').collect();
                if parts_m.len() >= 2 && parts_s.len() >= 2 && parts_m[1] == parts_s[1] {
                    return true;
                }
            }
        }

        false
    }

    /// Checks if a media file stem and sidecar stem match, supporting all standard Apple edit variants:
    /// `IMG_1234`, `IMG_E1234`, `IMG_O1234`, `OIMG_1234`.
    pub fn is_apple_sidecar_stem_match(media_stem: &str, sidecar_stem: &str) -> bool {
        let m = media_stem.to_ascii_lowercase();
        let s = sidecar_stem.to_ascii_lowercase();
        if m == s {
            return true;
        }

        fn normalize_apple_stem(stem: &str) -> &str {
            if let Some(rest) = stem.strip_prefix("img_e") {
                rest
            } else if let Some(rest) = stem.strip_prefix("img_o") {
                rest
            } else if let Some(rest) = stem.strip_prefix("oimg_") {
                rest
            } else if let Some(rest) = stem.strip_prefix("img_") {
                rest
            } else {
                stem
            }
        }

        let norm_m = normalize_apple_stem(&m);
        let norm_s = normalize_apple_stem(&s);

        !norm_m.is_empty() && norm_m == norm_s
    }

    /// Detects all companion sidecars (.AAE, .IFO, .BUP) on disk for a given media file path.
    pub fn find_companion_sidecars(media_path: &Path) -> Vec<PathBuf> {
        let parent = match media_path.parent() {
            Some(p) => p,
            None => return Vec::new(),
        };
        let stem = match media_path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => return Vec::new(),
        };

        let entries = match std::fs::read_dir(parent) {
            Ok(e) => e,
            Err(_) => return Vec::new(),
        };

        let mut sidecars = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if Self::is_sidecar_extension(ext) {
                    if let Some(entry_stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if Self::is_companion_sidecar_stem_match(stem, entry_stem, ext) {
                            sidecars.push(path);
                        }
                    }
                }
            }
        }

        sidecars.sort();
        sidecars.dedup();
        sidecars
    }

    /// Detects if a companion sidecar exists on disk for a given media file path.
    pub fn find_companion_sidecar(media_path: &Path) -> Option<PathBuf> {
        Self::find_companion_sidecars(media_path).into_iter().next()
    }

    /// Finds a companion media file (photo or video) on disk for a given sidecar.
    pub fn find_companion_media_file(sidecar_path: &Path) -> Option<PathBuf> {
        let parent = sidecar_path.parent()?;
        let sidecar_stem = sidecar_path.file_stem()?.to_str()?;
        let sidecar_ext = sidecar_path.extension()?.to_str().unwrap_or_default();

        let entries = std::fs::read_dir(parent).ok()?;
        let mut candidates = Vec::new();

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if !Self::is_sidecar_extension(ext) {
                    if let Some(entry_stem) = path.file_stem().and_then(|s| s.to_str()) {
                        if Self::is_companion_sidecar_stem_match(entry_stem, sidecar_stem, sidecar_ext) {
                            if entry_stem.eq_ignore_ascii_case(sidecar_stem) {
                                return Some(path);
                            }
                            candidates.push(path);
                        }
                    }
                }
            }
        }

        candidates.into_iter().next()
    }

    /// Transfers all companion sidecars (.aae, .ifo, .bup) alongside a primary media asset.
    /// Places each sidecar in the same target parent directory.
    pub async fn transfer_companion_sidecars_if_present(
        source_media_path: &Path,
        target_media_path: &Path,
        config: TransferBufferConfig,
        delete_source: bool,
    ) -> Result<Vec<crate::domain::dedup_models::SidecarTransferResult>, DeduplicationError> {
        let sidecars = Self::find_companion_sidecars(source_media_path);
        if sidecars.is_empty() {
            return Ok(Vec::new());
        }

        let target_parent = match target_media_path.parent() {
            Some(p) => p,
            None => {
                return Ok(Vec::new());
            }
        };

        let target_stem = target_media_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("media");

        let mut results = Vec::new();

        for sidecar_src in sidecars {
            let sidecar_ext = sidecar_src
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("sidecar");

            let is_dvd = sidecar_ext.eq_ignore_ascii_case("ifo") || sidecar_ext.eq_ignore_ascii_case("bup");
            let target_file_name = if is_dvd {
                sidecar_src
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("metadata.ifo")
                    .to_string()
            } else {
                format!("{}.{}", target_stem, sidecar_ext)
            };

            let target_sidecar_path = target_parent.join(target_file_name);

            let sidecar_size = std::fs::metadata(&sidecar_src)
                .map(|m| m.len())
                .unwrap_or(0);

            // If target already exists and has identical length, skip
            if target_sidecar_path.exists() {
                let existing_len = std::fs::metadata(&target_sidecar_path)
                    .map(|m| m.len())
                    .unwrap_or(0);
                if existing_len == sidecar_size {
                    if delete_source {
                        let _ = tokio::fs::remove_file(&sidecar_src).await;
                    }
                    results.push(crate::domain::dedup_models::SidecarTransferResult {
                        found: true,
                        source_path: Some(sidecar_src),
                        target_path: Some(target_sidecar_path),
                        bytes_transferred: 0,
                        skipped_identical: true,
                        error: None,
                    });
                    continue;
                }
            }

            // Perform stream copy verified
            let bytes_copied = Self::stream_copy_verified(
                &sidecar_src,
                &target_sidecar_path,
                sidecar_size,
                None,
                config,
            )
            .await?;

            if delete_source {
                let _ = tokio::fs::remove_file(&sidecar_src).await;
            }

            results.push(crate::domain::dedup_models::SidecarTransferResult {
                found: true,
                source_path: Some(sidecar_src),
                target_path: Some(target_sidecar_path),
                bytes_transferred: bytes_copied,
                skipped_identical: false,
                error: None,
            });
        }

        Ok(results)
    }

    /// Transfers an optional companion sidecar alongside a primary media asset.
    /// Places the sidecar in the same target parent directory.
    pub async fn transfer_companion_sidecar_if_present(
        source_media_path: &Path,
        target_media_path: &Path,
        config: TransferBufferConfig,
        delete_source: bool,
    ) -> Result<crate::domain::dedup_models::SidecarTransferResult, DeduplicationError> {
        let results = Self::transfer_companion_sidecars_if_present(
            source_media_path,
            target_media_path,
            config,
            delete_source,
        )
        .await?;
        Ok(results
            .into_iter()
            .next()
            .unwrap_or_default())
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
        let ext = Path::new(filename)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        let cat_folder = if category == MediaCategory::Sidecar
            && (ext.eq_ignore_ascii_case("ifo") || ext.eq_ignore_ascii_case("bup"))
        {
            "Videos"
        } else {
            Self::category_folder_name(category)
        };
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

        // Check if existing file is already identical in byte size and cryptographic hash (full or sample)
        let existing_len = std::fs::metadata(target_path).map(|m| m.len()).unwrap_or(0);

        if existing_len == source_size && source_hash.is_some() {
            if let Ok(existing_hash) = hasher.compute_cryptographic_hash(target_path) {
                if Some(existing_hash.as_str()) == source_hash {
                    return PathResolution::IdenticalExists(target_path.to_path_buf());
                }
            }
            if let Ok(existing_sample) = hasher.compute_sample_digest(target_path) {
                if Some(existing_sample.as_str()) == source_hash {
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

    /// Copies a file using a tuned asynchronous streaming buffer (`TransferBufferConfig`, default 8 MB),
    /// dynamic small-file bounding `min(expected_size, buffer_size)`, continuous in-flight BLAKE3
    /// hash computation, storage commit `sync_all()`, and purges the destination file if any
    /// verification fails.
    pub async fn stream_copy_verified(
        source: &Path,
        target: &Path,
        expected_size: u64,
        expected_hash: Option<&str>,
        config: TransferBufferConfig,
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

        let buf_size = config.effective_buffer_size(expected_size);
        let mut buffer = vec![0u8; buf_size];
        let mut total_copied = 0u64;

        let mut in_flight_hasher = if config.in_flight_hash && expected_hash.is_some() {
            Some(InFlightStreamHasher::new(expected_hash, expected_size))
        } else {
            None
        };

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

            if let Some(ref mut hasher) = in_flight_hasher {
                hasher.update(&buffer[..bytes_read]);
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
            .sync_all()
            .await
            .map_err(|e| DeduplicationError::TransferFailed {
                source_path: source.to_path_buf(),
                target_path: target.to_path_buf(),
                reason: format!("Sync error on target file: {}", e),
            })?;

        drop(dst_file);
        drop(src_file);

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

        // 2. Verify in-flight cryptographic hash
        if let Some(ref hasher) = in_flight_hasher {
            if let Err(e) = hasher.finalize_and_verify_with_paths(source, target) {
                let _ = tokio::fs::remove_file(target).await;
                return Err(e);
            }
        }

        // 3. Optional secondary disk re-read hash check (if explicitly requested)
        if config.recheck_disk_hash {
            let hasher = Blake3MediaHasher::new();
            match hasher.compute_cryptographic_hash(target) {
                Ok(dest_hash) => {
                    if let Some(exp_hash) = expected_hash {
                        if !exp_hash.is_empty() && exp_hash != dest_hash {
                            let sample_match = hasher
                                .compute_sample_digest(target)
                                .map(|s| s == exp_hash)
                                .unwrap_or(false);
                            if !sample_match {
                                let _ = tokio::fs::remove_file(target).await;
                                return Err(DeduplicationError::TransferFailed {
                                    source_path: source.to_path_buf(),
                                    target_path: target.to_path_buf(),
                                    reason: format!(
                                        "Secondary disk re-read hash mismatch on '{}': expected {}, got {}",
                                        target.display(),
                                        exp_hash,
                                        dest_hash
                                    ),
                                });
                            }
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
