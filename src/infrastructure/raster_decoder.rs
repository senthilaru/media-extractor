use std::path::Path;

use crate::domain::errors::FingerprintError;

/// Bounded image raster decoder generating 32x32 grayscale thumbnails for perceptual hashing.
#[derive(Debug, Default, Clone)]
pub struct RasterDecoder;

impl RasterDecoder {
    pub fn new() -> Self {
        Self
    }

    /// Decodes an image file and scales it to a 32x32 8-bit grayscale matrix.
    pub fn decode_thumbnail_32x32(path: &Path) -> Result<[u8; 1024], FingerprintError> {
        let reader = image::ImageReader::open(path).map_err(|e| FingerprintError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;

        let format_guessed =
            reader
                .with_guessed_format()
                .map_err(|e| FingerprintError::ImageDecode {
                    path: path.to_path_buf(),
                    message: format!("Failed to determine image format: {e}"),
                })?;

        let dynamic_image = format_guessed
            .decode()
            .map_err(|e| FingerprintError::ImageDecode {
                path: path.to_path_buf(),
                message: format!("Image decode failure: {e}"),
            })?;

        // Resize directly to 32x32 with fast filter
        let resized = dynamic_image.resize_exact(32, 32, image::imageops::FilterType::Triangle);
        let luma = resized.to_luma8();
        let raw_bytes = luma.as_raw();

        if raw_bytes.len() < 1024 {
            return Err(FingerprintError::ImageDecode {
                path: path.to_path_buf(),
                message: "Decoded buffer smaller than 32x32 (1024 bytes)".to_string(),
            });
        }

        let mut matrix = [0u8; 1024];
        matrix.copy_from_slice(&raw_bytes[..1024]);
        Ok(matrix)
    }
}
