use phf::phf_map;
use std::path::Path;

use crate::domain::models::MediaCategory;
use crate::domain::ports::MediaClassifier;

static EXTENSION_MAP: phf::Map<&'static str, MediaCategory> = phf_map! {
    // Compressed photos
    "jpg" => MediaCategory::Photo,
    "jpeg" => MediaCategory::Photo,
    "png" => MediaCategory::Photo,
    "webp" => MediaCategory::Photo,
    "gif" => MediaCategory::Photo,
    "bmp" => MediaCategory::Photo,
    "heic" => MediaCategory::Photo,
    "heif" => MediaCategory::Photo,
    "avif" => MediaCategory::Photo,
    "tiff" => MediaCategory::Photo,
    "tif" => MediaCategory::Photo,
    "mp" => MediaCategory::Photo,

    // Videos
    "mp4" => MediaCategory::Video,
    "mov" => MediaCategory::Video,
    "mkv" => MediaCategory::Video,
    "avi" => MediaCategory::Video,
    "webm" => MediaCategory::Video,
    "m4v" => MediaCategory::Video,
    "3gp" => MediaCategory::Video,
    "wmv" => MediaCategory::Video,
    "flv" => MediaCategory::Video,
    "mts" => MediaCategory::Video,
    "m2ts" => MediaCategory::Video,

    // Camera RAW formats
    "cr2" => MediaCategory::RawPhoto,
    "cr3" => MediaCategory::RawPhoto,
    "nef" => MediaCategory::RawPhoto,
    "arw" => MediaCategory::RawPhoto,
    "dng" => MediaCategory::RawPhoto,
    "orf" => MediaCategory::RawPhoto,
    "rw2" => MediaCategory::RawPhoto,
    "pef" => MediaCategory::RawPhoto,
    "raf" => MediaCategory::RawPhoto,
    "srw" => MediaCategory::RawPhoto,
};

/// High-performance media classifier with zero heap allocations on the hot path.
#[derive(Debug, Default, Clone)]
pub struct PhfMediaClassifier;

impl PhfMediaClassifier {
    pub fn new() -> Self {
        Self
    }

    /// Helper to lowercase an ASCII extension into a fixed stack buffer without heap allocation.
    #[inline]
    fn to_lowercase_ascii(ext: &str, buf: &mut [u8; 16]) -> Option<usize> {
        let bytes = ext.as_bytes();
        if bytes.is_empty() || bytes.len() > buf.len() {
            return None;
        }
        for (i, &b) in bytes.iter().enumerate() {
            buf[i] = b.to_ascii_lowercase();
        }
        Some(bytes.len())
    }
}

impl MediaClassifier for PhfMediaClassifier {
    fn classify(&self, path: &Path) -> Option<MediaCategory> {
        let ext_os = path.extension()?;
        let ext_str = ext_os.to_str()?;

        let mut buf = [0u8; 16];
        let len = Self::to_lowercase_ascii(ext_str, &mut buf)?;
        let lower_str = std::str::from_utf8(&buf[..len]).ok()?;

        EXTENSION_MAP.get(lower_str).copied()
    }

    fn is_supported_extension(&self, extension: &str) -> bool {
        let mut buf = [0u8; 16];
        if let Some(len) = Self::to_lowercase_ascii(extension, &mut buf) {
            if let Ok(lower_str) = std::str::from_utf8(&buf[..len]) {
                return EXTENSION_MAP.contains_key(lower_str);
            }
        }
        false
    }
}
