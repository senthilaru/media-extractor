use std::path::Path;
use std::time::SystemTime;
use uuid::Uuid;

use nom_exif::{Exif, ExifTag, MediaParser, MediaSource};

use crate::domain::errors::ExtractionError;
use crate::domain::metadata_models::{
    CameraMetadata, ExposureMetadata, ExtractionResultKind, ExtractionStatus, FileContext,
    GeoLocation, ImageDimensions, MediaMetadataDocument, TemporalMetadata, TimestampSource,
};
use crate::domain::models::DiscoveredMediaItem;
use crate::domain::ports::MediaMetadataExtractor;

/// Metadata extractor adapter specialized for photos and camera RAW formats.
#[derive(Debug, Default, Clone)]
pub struct ExifMetadataExtractor;

impl ExifMetadataExtractor {
    pub fn new() -> Self {
        Self
    }

    /// Attempts to parse EXIF using `nom-exif`.
    fn try_parse_nom_exif(path: &Path) -> Option<Exif> {
        let ms = MediaSource::open(path).ok()?;
        let mut parser = MediaParser::new();
        let iter = parser.parse_exif(ms).ok()?;
        Some(iter.into())
    }

    /// Fallback parser using `kamadak-exif` for TIFF/RAW files if `nom-exif` encounters unsupported structures.
    fn try_parse_kamadak_exif(path: &Path) -> Option<exif::Exif> {
        let file = std::fs::File::open(path).ok()?;
        let mut bufreader = std::io::BufReader::new(file);
        let exifreader = exif::Reader::new();
        exifreader.read_from_container(&mut bufreader).ok()
    }

    /// Extracts full sensor image dimensions from TIFF SubIFDs for RAW files (such as Nikon NEF).
    /// Nikon NEFs place thumbnail dimensions (e.g. 160x120) in IFD0, while full sensor dimensions
    /// (e.g. 6000x4000) are stored in SubIFDs (tag 0x014A).
    pub fn try_parse_tiff_subifd_dimensions(path: &Path) -> Option<(u32, u32)> {
        use std::io::{Read, Seek, SeekFrom};
        let mut file = std::fs::File::open(path).ok()?;
        let mut header = [0u8; 8];
        file.read_exact(&mut header).ok()?;

        let is_le = match &header[0..2] {
            b"II" => true,
            b"MM" => false,
            _ => return None,
        };

        let magic = if is_le {
            u16::from_le_bytes([header[2], header[3]])
        } else {
            u16::from_be_bytes([header[2], header[3]])
        };
        if magic != 42 {
            return None;
        }

        let ifd0_offset = if is_le {
            u32::from_le_bytes([header[4], header[5], header[6], header[7]])
        } else {
            u32::from_be_bytes([header[4], header[5], header[6], header[7]])
        };

        let read_u16 = |buf: &[u8]| -> u16 {
            if is_le {
                u16::from_le_bytes([buf[0], buf[1]])
            } else {
                u16::from_be_bytes([buf[0], buf[1]])
            }
        };
        let read_u32 = |buf: &[u8]| -> u32 {
            if is_le {
                u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])
            } else {
                u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]])
            }
        };

        // Read IFD0 to discover SubIFDs tag (0x014A)
        file.seek(SeekFrom::Start(ifd0_offset as u64)).ok()?;
        let mut num_entries_buf = [0u8; 2];
        file.read_exact(&mut num_entries_buf).ok()?;
        let num_entries = read_u16(&num_entries_buf);

        let mut subifd_offsets = Vec::new();
        for _ in 0..num_entries {
            let mut entry = [0u8; 12];
            if file.read_exact(&mut entry).is_err() {
                break;
            }
            let tag = read_u16(&entry[0..2]);
            let count = read_u32(&entry[4..8]);
            let val_or_offset = read_u32(&entry[8..12]);

            if tag == 0x014A {
                if count == 1 {
                    subifd_offsets.push(val_or_offset);
                } else if count > 1 {
                    let cur_pos = file.stream_position().ok()?;
                    if file.seek(SeekFrom::Start(val_or_offset as u64)).is_ok() {
                        for _ in 0..count.min(10) {
                            let mut off_buf = [0u8; 4];
                            if file.read_exact(&mut off_buf).is_ok() {
                                subifd_offsets.push(read_u32(&off_buf));
                            }
                        }
                        let _ = file.seek(SeekFrom::Start(cur_pos));
                    }
                }
            }
        }

        let mut max_pixels = 0u64;
        let mut best_dims = None;

        // Inspect each SubIFD for ImageWidth (0x0100) and ImageLength (0x0101)
        for offset in subifd_offsets {
            if file.seek(SeekFrom::Start(offset as u64)).is_err() {
                continue;
            }
            let mut sub_entries_buf = [0u8; 2];
            if file.read_exact(&mut sub_entries_buf).is_err() {
                continue;
            }
            let sub_entries = read_u16(&sub_entries_buf);

            let mut w = None;
            let mut h = None;

            for _ in 0..sub_entries {
                let mut entry = [0u8; 12];
                if file.read_exact(&mut entry).is_err() {
                    break;
                }
                let tag = read_u16(&entry[0..2]);
                let entry_type = read_u16(&entry[2..4]);
                let val_or_offset = read_u32(&entry[8..12]);

                let val = if entry_type == 3 {
                    // Short
                    if is_le {
                        read_u16(&entry[8..10]) as u32
                    } else {
                        read_u16(&entry[8..10]) as u32
                    }
                } else {
                    val_or_offset
                };

                if tag == 0x0100 {
                    w = Some(val);
                } else if tag == 0x0101 {
                    h = Some(val);
                }
            }

            if let (Some(width), Some(height)) = (w, h) {
                let pixels = (width as u64) * (height as u64);
                if pixels > max_pixels {
                    max_pixels = pixels;
                    best_dims = Some((width, height));
                }
            }
        }

        best_dims
    }
}

fn entry_value_to_f64(v: &nom_exif::EntryValue) -> Option<f64> {
    match v {
        nom_exif::EntryValue::F64(f) => Some(*f),
        nom_exif::EntryValue::F32(f) => Some(*f as f64),
        nom_exif::EntryValue::URational(r) => r.to_f64(),
        nom_exif::EntryValue::IRational(r) => r.to_f64(),
        nom_exif::EntryValue::U32(u) => Some(*u as f64),
        nom_exif::EntryValue::U16(u) => Some(*u as f64),
        _ => None,
    }
}

fn kamadak_field_f64(field: &exif::Field) -> Option<f64> {
    match &field.value {
        exif::Value::Rational(v) => v.first().map(|r| r.to_f64()),
        exif::Value::SRational(v) => v.first().map(|r| r.to_f64()),
        exif::Value::Short(v) => v.first().map(|u| *u as f64),
        exif::Value::Long(v) => v.first().map(|u| *u as f64),
        _ => None,
    }
}

fn kamadak_field_u32(field: &exif::Field) -> Option<u32> {
    match &field.value {
        exif::Value::Short(v) => v.first().map(|u| *u as u32),
        exif::Value::Long(v) => v.first().copied(),
        _ => None,
    }
}

fn kamadak_field_clean_str(field: &exif::Field) -> Option<String> {
    let s = field.display_value().to_string();
    let trimmed = s.trim().trim_matches('"').trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

impl MediaMetadataExtractor for ExifMetadataExtractor {
    fn extract_file(
        &self,
        item: &DiscoveredMediaItem,
        scanned_root: Option<&Path>,
    ) -> Result<MediaMetadataDocument, ExtractionError> {
        let _span = tracing::debug_span!("exif_extract_file", path = %item.path.display());
        let _enter = _span.enter();

        let path = &item.path;
        if !path.exists() {
            return Err(ExtractionError::FileNotFound(path.clone()));
        }

        let fs_metadata = std::fs::metadata(path).map_err(|e| ExtractionError::Io {
            path: path.clone(),
            source: e,
        })?;

        // Determine directory contexts
        let root = scanned_root
            .map(|p| p.to_path_buf())
            .or_else(|| path.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| path.clone());

        let relative_path = path.strip_prefix(&root).unwrap_or(path).to_path_buf();

        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();

        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let modified_at = item
            .modified_at
            .or_else(|| std::fs::metadata(path).ok()?.modified().ok())
            .map(|st| st.into());

        let file_context = FileContext {
            scanned_root: root,
            file_path: path.clone(),
            relative_path,
            file_name,
            file_size_bytes: item.file_size_bytes,
            extension,
            media_category: item.category,
            modified_at,
        };

        let mut warnings = Vec::new();
        let mut extraction_status_kind = ExtractionResultKind::Success;

        // Try primary parser: nom-exif
        let nom_exif_opt = Self::try_parse_nom_exif(path);

        let mut temporal: Option<TemporalMetadata> = None;
        let mut camera: Option<CameraMetadata> = None;
        let mut exposure: Option<ExposureMetadata> = None;
        let mut location: Option<GeoLocation> = None;
        let mut dimensions: Option<ImageDimensions> = None;

        if let Some(ref exif) = nom_exif_opt {
            // 1. Temporal extraction
            if let Some(val) = exif.get(ExifTag::DateTimeOriginal) {
                if let Some(exif_dt) = val.as_datetime() {
                    temporal = Some(TemporalMetadata::from_utc_datetime(
                        exif_dt.into_naive().and_utc(),
                        TimestampSource::ExifDateTimeOriginal,
                    ));
                } else if let Some(s) = val.as_str() {
                    temporal =
                        TemporalMetadata::parse_exif_date(s, TimestampSource::ExifDateTimeOriginal);
                }
            }

            if temporal.is_none() {
                if let Some(val) = exif.get(ExifTag::CreateDate) {
                    if let Some(exif_dt) = val.as_datetime() {
                        temporal = Some(TemporalMetadata::from_utc_datetime(
                            exif_dt.into_naive().and_utc(),
                            TimestampSource::ExifCreateDate,
                        ));
                    } else if let Some(s) = val.as_str() {
                        temporal =
                            TemporalMetadata::parse_exif_date(s, TimestampSource::ExifCreateDate);
                    }
                }
            }

            // 2. Camera extraction
            let make = exif
                .get(ExifTag::Make)
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            let model = exif
                .get(ExifTag::Model)
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            let lens_model = exif
                .get(ExifTag::LensModel)
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            let software = exif
                .get(ExifTag::Software)
                .and_then(|v| v.as_str())
                .map(ToString::to_string);

            if make.is_some() || model.is_some() || lens_model.is_some() || software.is_some() {
                camera = Some(CameraMetadata {
                    make,
                    model,
                    lens_model,
                    software,
                });
            }

            // 3. Exposure extraction
            let focal_length_mm = exif.get(ExifTag::FocalLength).and_then(entry_value_to_f64);
            let focal_length_35mm = exif
                .get(ExifTag::FocalLengthIn35mmFilm)
                .and_then(entry_value_to_f64);
            let f_number = exif.get(ExifTag::FNumber).and_then(entry_value_to_f64);
            let exposure_time_seconds =
                exif.get(ExifTag::ExposureTime).and_then(entry_value_to_f64);
            let exposure_time_str = exposure_time_seconds.map(|sec| {
                if sec > 0.0 && sec < 1.0 {
                    let denom = (1.0 / sec).round() as u64;
                    format!("1/{denom}")
                } else {
                    format!("{sec:.3}")
                }
            });
            let iso = exif.get(ExifTag::ISOSpeedRatings).and_then(|v| v.as_u32());
            let flash_fired = exif
                .get(ExifTag::Flash)
                .and_then(|v| v.as_u32().or_else(|| v.as_u16().map(|u| u as u32)))
                .map(|v| (v & 1) != 0);

            if focal_length_mm.is_some()
                || f_number.is_some()
                || exposure_time_seconds.is_some()
                || iso.is_some()
            {
                exposure = Some(ExposureMetadata {
                    focal_length_mm,
                    focal_length_35mm,
                    f_number,
                    exposure_time_seconds,
                    exposure_time_str,
                    iso,
                    flash_fired,
                });
            }

            // 4. Dimensions & Orientation
            let width = exif
                .get(ExifTag::ExifImageWidth)
                .or_else(|| exif.get(ExifTag::ImageWidth))
                .and_then(|v| v.as_u32().or_else(|| v.as_u16().map(|u| u as u32)));

            let height = exif
                .get(ExifTag::ExifImageHeight)
                .or_else(|| exif.get(ExifTag::ImageHeight))
                .and_then(|v| v.as_u32().or_else(|| v.as_u16().map(|u| u as u32)));

            let orientation = exif.get(ExifTag::Orientation).and_then(|v| v.as_u16());

            if let (Some(w), Some(h)) = (width, height) {
                dimensions = Some(ImageDimensions {
                    width: w,
                    height: h,
                    orientation,
                });
            }

            // 5. GPS Location
            if let Some(gps) = exif.gps_info() {
                let lat_deg = gps.latitude.to_decimal_degrees();
                let lon_deg = gps.longitude.to_decimal_degrees();
                if let (Some(lat_val), Some(lon_val)) = (lat_deg, lon_deg) {
                    let lat = lat_val * gps.latitude_ref.sign();
                    let lon = lon_val * gps.longitude_ref.sign();
                    let alt = gps.altitude.meters();
                    location = GeoLocation::new(lat, lon, alt);
                }
            }
        }

        // Secondary fallback to kamadak-exif for RAW and TIFF files if attributes are missing
        if temporal.is_none() || camera.is_none() || exposure.is_none() || dimensions.is_none() {
            if let Some(k_exif) = Self::try_parse_kamadak_exif(path) {
                tracing::debug!(path = %path.display(), "Inspecting secondary kamadak-exif tags");

                // Temporal fallback
                if temporal.is_none() {
                    if let Some(field) = k_exif
                        .get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY)
                        .or_else(|| {
                            k_exif.get_field(exif::Tag::DateTimeDigitized, exif::In::PRIMARY)
                        })
                        .or_else(|| k_exif.get_field(exif::Tag::DateTime, exif::In::PRIMARY))
                    {
                        let val_str = field.display_value().to_string();
                        temporal = TemporalMetadata::parse_exif_date(
                            &val_str,
                            TimestampSource::ExifDateTimeOriginal,
                        );
                    }
                }

                // Camera fallback
                if camera.is_none() {
                    let make = k_exif
                        .get_field(exif::Tag::Make, exif::In::PRIMARY)
                        .and_then(kamadak_field_clean_str);
                    let model = k_exif
                        .get_field(exif::Tag::Model, exif::In::PRIMARY)
                        .and_then(kamadak_field_clean_str);
                    let lens_model = k_exif
                        .get_field(exif::Tag::LensModel, exif::In::PRIMARY)
                        .and_then(kamadak_field_clean_str);
                    let software = k_exif
                        .get_field(exif::Tag::Software, exif::In::PRIMARY)
                        .and_then(kamadak_field_clean_str);

                    if make.is_some()
                        || model.is_some()
                        || lens_model.is_some()
                        || software.is_some()
                    {
                        camera = Some(CameraMetadata {
                            make,
                            model,
                            lens_model,
                            software,
                        });
                    }
                }

                // Exposure fallback
                if exposure.is_none() {
                    let focal_length_mm = k_exif
                        .get_field(exif::Tag::FocalLength, exif::In::PRIMARY)
                        .and_then(kamadak_field_f64);
                    let focal_length_35mm = k_exif
                        .get_field(exif::Tag::FocalLengthIn35mmFilm, exif::In::PRIMARY)
                        .and_then(kamadak_field_f64);
                    let f_number = k_exif
                        .get_field(exif::Tag::FNumber, exif::In::PRIMARY)
                        .and_then(kamadak_field_f64);
                    let exposure_time_seconds = k_exif
                        .get_field(exif::Tag::ExposureTime, exif::In::PRIMARY)
                        .and_then(kamadak_field_f64);
                    let exposure_time_str = exposure_time_seconds.map(|sec| {
                        if sec > 0.0 && sec < 1.0 {
                            let denom = (1.0 / sec).round() as u64;
                            format!("1/{denom}")
                        } else {
                            format!("{sec:.3}")
                        }
                    });
                    let iso = k_exif
                        .get_field(exif::Tag::PhotographicSensitivity, exif::In::PRIMARY)
                        .and_then(kamadak_field_u32);
                    let flash_fired = k_exif
                        .get_field(exif::Tag::Flash, exif::In::PRIMARY)
                        .and_then(kamadak_field_u32)
                        .map(|v| (v & 1) != 0);

                    if focal_length_mm.is_some()
                        || f_number.is_some()
                        || exposure_time_seconds.is_some()
                        || iso.is_some()
                    {
                        exposure = Some(ExposureMetadata {
                            focal_length_mm,
                            focal_length_35mm,
                            f_number,
                            exposure_time_seconds,
                            exposure_time_str,
                            iso,
                            flash_fired,
                        });
                    }
                }

                // Dimensions fallback
                if dimensions.is_none() {
                    let width = k_exif
                        .get_field(exif::Tag::PixelXDimension, exif::In::PRIMARY)
                        .or_else(|| k_exif.get_field(exif::Tag::ImageWidth, exif::In::PRIMARY))
                        .and_then(kamadak_field_u32);
                    let height = k_exif
                        .get_field(exif::Tag::PixelYDimension, exif::In::PRIMARY)
                        .or_else(|| k_exif.get_field(exif::Tag::ImageLength, exif::In::PRIMARY))
                        .and_then(kamadak_field_u32);
                    let orientation = k_exif
                        .get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                        .and_then(kamadak_field_u32)
                        .map(|u| u as u16);

                    if let (Some(w), Some(h)) = (width, height) {
                        dimensions = Some(ImageDimensions {
                            width: w,
                            height: h,
                            orientation,
                        });
                    }
                }
            }
        }

        // For RAW/NEF files or if extracted dimensions are thumbnail-sized (<= 320px) while file > 1MB,
        // inspect TIFF SubIFDs for full-resolution raw sensor dimensions.
        let is_raw_format = item.category == crate::domain::models::MediaCategory::RawPhoto
            || item.extension.eq_ignore_ascii_case("nef");
        let current_pixels = dimensions
            .as_ref()
            .map(|d| (d.width as u64) * (d.height as u64))
            .unwrap_or(0);
        let is_small_dim = dimensions
            .as_ref()
            .map(|d| d.width <= 320 || d.height <= 320)
            .unwrap_or(true);

        if (is_raw_format || is_small_dim) && fs_metadata.len() > 1_000_000 {
            if let Some((sub_w, sub_h)) = Self::try_parse_tiff_subifd_dimensions(path) {
                let sub_pixels = (sub_w as u64) * (sub_h as u64);
                if sub_pixels > current_pixels {
                    let orientation = dimensions.and_then(|d| d.orientation);
                    dimensions = Some(ImageDimensions {
                        width: sub_w,
                        height: sub_h,
                        orientation,
                    });
                }
            }
        }

        // Fallback to filesystem timestamp if no embedded timestamp was discovered
        let final_temporal = match temporal {
            Some(t) => t,
            None => {
                warnings.push("No embedded EXIF capture datetime found; fell back to filesystem modified time".to_string());
                extraction_status_kind = ExtractionResultKind::Partial;
                let modified = fs_metadata.modified().unwrap_or(SystemTime::now());
                TemporalMetadata::from_system_time(modified)
            }
        };

        Ok(MediaMetadataDocument {
            id: Uuid::new_v4(),
            file_context,
            temporal: final_temporal,
            camera,
            exposure,
            location,
            dimensions,
            video: None,
            fingerprint: None,
            extraction_status: ExtractionStatus {
                status: extraction_status_kind,
                warnings,
                error_message: None,
            },
            dedup: None,
            visual: None,
        })
    }
}
