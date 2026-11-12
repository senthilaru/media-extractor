use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::SystemTime;
use uuid::Uuid;

use nom_exif::{MediaParser, MediaSource, TrackInfo, TrackInfoTag};

use crate::domain::errors::ExtractionError;
use crate::domain::metadata_models::{
    CameraMetadata, ExtractionResultKind, ExtractionStatus, FileContext, GeoLocation,
    ImageDimensions, MediaMetadataDocument, TemporalMetadata, TimestampSource, VideoMetadata,
};
use crate::domain::models::DiscoveredMediaItem;
use crate::domain::ports::MediaMetadataExtractor;

#[derive(Debug, Default)]
struct ContainerTrackDetails {
    rotation_degrees: Option<i32>,
    video_width: Option<u32>,
    video_height: Option<u32>,
    video_codec: Option<String>,
    audio_codec: Option<String>,
    frame_rate_fps: Option<f64>,
    current_timescale: Option<u32>,
}

fn read_box_header<R: Read + Seek>(reader: &mut R) -> std::io::Result<Option<(u64, [u8; 4], u64)>> {
    let mut header = [0u8; 8];
    match reader.read_exact(&mut header) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }

    let size32 = u32::from_be_bytes([header[0], header[1], header[2], header[3]]);
    let box_type = [header[4], header[5], header[6], header[7]];

    if size32 == 1 {
        let mut large_buf = [0u8; 8];
        reader.read_exact(&mut large_buf)?;
        let size64 = u64::from_be_bytes(large_buf);
        Ok(Some((size64, box_type, 16)))
    } else if size32 == 0 {
        let current = reader.stream_position()?;
        let end = reader.seek(SeekFrom::End(0))?;
        reader.seek(SeekFrom::Start(current))?;
        let size = end.saturating_sub(current.saturating_sub(8));
        Ok(Some((size, box_type, 8)))
    } else {
        Ok(Some((size32 as u64, box_type, 8)))
    }
}

fn inspect_container_atoms(path: &Path) -> ContainerTrackDetails {
    let mut details = ContainerTrackDetails::default();
    let Ok(mut file) = File::open(path) else {
        return details;
    };
    let Ok(file_len) = file.metadata().map(|m| m.len()) else {
        return details;
    };
    if file_len < 8 {
        return details;
    }

    let _ = scan_boxes(&mut file, 0, file_len, 0, &mut details);
    details
}

fn scan_boxes<R: Read + Seek>(
    reader: &mut R,
    start: u64,
    end: u64,
    depth: usize,
    details: &mut ContainerTrackDetails,
) -> std::io::Result<()> {
    if depth > 10 {
        return Ok(());
    }
    let mut offset = start;

    while offset + 8 <= end {
        reader.seek(SeekFrom::Start(offset))?;
        let Some((box_size, box_type, header_len)) = read_box_header(reader)? else {
            break;
        };

        if box_size < header_len || offset + box_size > end {
            break;
        }

        let payload_start = offset + header_len;
        let payload_end = offset + box_size;

        match &box_type {
            b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" => {
                scan_boxes(reader, payload_start, payload_end, depth + 1, details)?;
            }
            b"tkhd" => {
                parse_tkhd(reader, payload_start, details)?;
            }
            b"mdhd" => {
                parse_mdhd(reader, payload_start, details)?;
            }
            b"stsd" => {
                parse_stsd(reader, payload_start, details)?;
            }
            b"stts" => {
                parse_stts(reader, payload_start, details)?;
            }
            _ => {
                // Skip all other boxes without buffering intermediate video stream bytes
            }
        }

        offset += box_size;
    }

    Ok(())
}

fn parse_tkhd<R: Read + Seek>(
    reader: &mut R,
    payload_start: u64,
    details: &mut ContainerTrackDetails,
) -> std::io::Result<()> {
    reader.seek(SeekFrom::Start(payload_start))?;
    let mut ver_flags = [0u8; 4];
    reader.read_exact(&mut ver_flags)?;
    let version = ver_flags[0];

    let skip_to_matrix = if version == 1 { 48 } else { 36 };
    reader.seek(SeekFrom::Current(skip_to_matrix))?;

    let mut matrix_and_dims = [0u8; 44];
    if reader.read_exact(&mut matrix_and_dims).is_ok() {
        let a = i32::from_be_bytes(matrix_and_dims[0..4].try_into().unwrap_or_default());
        let b = i32::from_be_bytes(matrix_and_dims[4..8].try_into().unwrap_or_default());
        let c = i32::from_be_bytes(matrix_and_dims[12..16].try_into().unwrap_or_default());
        let d = i32::from_be_bytes(matrix_and_dims[16..20].try_into().unwrap_or_default());

        let rotation = if a == 0 && b == 65536 && c == -65536 && d == 0 {
            Some(90)
        } else if a == -65536 && b == 0 && c == 0 && d == -65536 {
            Some(180)
        } else if a == 0 && b == -65536 && c == 65536 && d == 0 {
            Some(270)
        } else if a == 65536 && b == 0 && c == 0 && d == 65536 {
            Some(0)
        } else {
            None
        };

        if let Some(rot) = rotation {
            if details.rotation_degrees.is_none() || rot != 0 {
                details.rotation_degrees = Some(rot);
            }
        }

        let w_fixed = u32::from_be_bytes(matrix_and_dims[36..40].try_into().unwrap_or_default());
        let h_fixed = u32::from_be_bytes(matrix_and_dims[40..44].try_into().unwrap_or_default());
        let w = w_fixed >> 16;
        let h = h_fixed >> 16;
        if w > 0 && h > 0 {
            details.video_width = Some(w);
            details.video_height = Some(h);
        }
    }
    Ok(())
}

fn parse_mdhd<R: Read + Seek>(
    reader: &mut R,
    payload_start: u64,
    details: &mut ContainerTrackDetails,
) -> std::io::Result<()> {
    reader.seek(SeekFrom::Start(payload_start))?;
    let mut ver_flags = [0u8; 4];
    reader.read_exact(&mut ver_flags)?;
    let version = ver_flags[0];

    let skip_to_timescale = if version == 1 { 16 } else { 8 };
    reader.seek(SeekFrom::Current(skip_to_timescale))?;

    let mut buf = [0u8; 4];
    reader.read_exact(&mut buf)?;
    let timescale = u32::from_be_bytes(buf);
    if timescale > 0 {
        details.current_timescale = Some(timescale);
    }
    Ok(())
}

fn parse_stts<R: Read + Seek>(
    reader: &mut R,
    payload_start: u64,
    details: &mut ContainerTrackDetails,
) -> std::io::Result<()> {
    reader.seek(SeekFrom::Start(payload_start))?;
    let mut header = [0u8; 8];
    reader.read_exact(&mut header)?;
    let entry_count = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);

    if entry_count > 0 {
        let mut entry = [0u8; 8];
        if reader.read_exact(&mut entry).is_ok() {
            let sample_delta = u32::from_be_bytes([entry[4], entry[5], entry[6], entry[7]]);
            if let Some(ts) = details.current_timescale {
                if ts > 0 && sample_delta > 0 && details.frame_rate_fps.is_none() {
                    let fps = (ts as f64) / (sample_delta as f64);
                    if (1.0..=240.0).contains(&fps) {
                        details.frame_rate_fps = Some((fps * 100.0).round() / 100.0);
                    }
                }
            }
        }
    }
    Ok(())
}

fn parse_stsd<R: Read + Seek>(
    reader: &mut R,
    payload_start: u64,
    details: &mut ContainerTrackDetails,
) -> std::io::Result<()> {
    reader.seek(SeekFrom::Start(payload_start))?;
    let mut header = [0u8; 8];
    reader.read_exact(&mut header)?;
    let entry_count = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);

    for _ in 0..entry_count.min(5) {
        let mut entry_hdr = [0u8; 8];
        if reader.read_exact(&mut entry_hdr).is_err() {
            break;
        }
        let entry_size =
            u32::from_be_bytes([entry_hdr[0], entry_hdr[1], entry_hdr[2], entry_hdr[3]]) as u64;
        let format = &entry_hdr[4..8];

        match format {
            b"avc1" => details.video_codec = Some("h264".to_string()),
            b"hvc1" | b"hev1" => details.video_codec = Some("hevc".to_string()),
            b"vp09" => details.video_codec = Some("vp9".to_string()),
            b"av01" => details.video_codec = Some("av1".to_string()),
            b"mp4v" => details.video_codec = Some("mpeg4".to_string()),
            b"apch" | b"apcn" | b"apcs" | b"apco" | b"ap4h" => {
                details.video_codec = Some("prores".to_string())
            }
            b"mp4a" => details.audio_codec = Some("aac".to_string()),
            b"alac" => details.audio_codec = Some("alac".to_string()),
            b"Opus" | b"opus" => details.audio_codec = Some("opus".to_string()),
            b"fLaC" | b"flac" => details.audio_codec = Some("flac".to_string()),
            _ => {}
        }

        if entry_size >= 8 {
            let _ = reader.seek(SeekFrom::Current((entry_size - 8) as i64));
        }
    }
    Ok(())
}

/// Metadata extractor adapter specialized for video containers (MP4, MOV, MKV, WebM, 3GP).
#[derive(Debug, Default, Clone)]
pub struct VideoMetadataExtractor;

impl VideoMetadataExtractor {
    pub fn new() -> Self {
        Self
    }

    /// Attempts to parse container tracks using `nom-exif` with streaming seekable reader.
    fn try_parse_track(path: &Path) -> Option<TrackInfo> {
        let ms = MediaSource::open(path).ok()?;
        let mut parser = MediaParser::new();
        parser.parse_track(ms).ok()
    }
}

impl MediaMetadataExtractor for VideoMetadataExtractor {
    fn extract_file(
        &self,
        item: &DiscoveredMediaItem,
        scanned_root: Option<&Path>,
    ) -> Result<MediaMetadataDocument, ExtractionError> {
        let _span = tracing::debug_span!("video_extract_file", path = %item.path.display());
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
            extension: extension.clone(),
            media_category: item.category,
            modified_at,
        };

        let mut warnings = Vec::new();
        let mut extraction_status_kind = ExtractionResultKind::Success;

        let track_info_opt = Self::try_parse_track(path);
        let container_details = inspect_container_atoms(path);
        let rotation = container_details.rotation_degrees.unwrap_or(0);

        let mut temporal: Option<TemporalMetadata> = None;
        let mut camera: Option<CameraMetadata> = None;
        let mut location: Option<GeoLocation> = None;
        let mut dimensions: Option<ImageDimensions> = None;
        let mut video: Option<VideoMetadata> = None;

        if let Some(ref track) = track_info_opt {
            // 1. Temporal extraction from CreateDate
            if let Some(val) = track.get(TrackInfoTag::CreateDate) {
                if let Some(exif_dt) = val.as_datetime() {
                    temporal = Some(TemporalMetadata::from_utc_datetime(
                        exif_dt.into_naive().and_utc(),
                        TimestampSource::QuickTimeCreationTime,
                    ));
                } else if let Some(s) = val.as_str() {
                    temporal = TemporalMetadata::parse_exif_date(
                        s,
                        TimestampSource::QuickTimeCreationTime,
                    );
                }
            }

            // 2. Hardware specs (Make, Model, Software)
            let make = track
                .get(TrackInfoTag::Make)
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            let model = track
                .get(TrackInfoTag::Model)
                .and_then(|v| v.as_str())
                .map(ToString::to_string);
            let software = track
                .get(TrackInfoTag::Software)
                .and_then(|v| v.as_str())
                .map(ToString::to_string);

            if make.is_some() || model.is_some() || software.is_some() {
                camera = Some(CameraMetadata {
                    make,
                    model,
                    lens_model: None,
                    software,
                });
            }

            // 3. Display dimensions
            let mut width = track
                .get(TrackInfoTag::Width)
                .and_then(|v| v.as_u32())
                .or(container_details.video_width);
            let mut height = track
                .get(TrackInfoTag::Height)
                .and_then(|v| v.as_u32())
                .or(container_details.video_height);

            // Effective display dimensions: swap width and height when rotated 90 or 270 degrees
            if (rotation == 90 || rotation == 270) && width.is_some() && height.is_some() {
                std::mem::swap(&mut width, &mut height);
            }

            if let (Some(w), Some(h)) = (width, height) {
                dimensions = Some(ImageDimensions {
                    width: w,
                    height: h,
                    orientation: Some(rotation as u16),
                });
            }

            // 4. Video container specific properties
            let duration_ms = track
                .get(TrackInfoTag::DurationMs)
                .and_then(|v| v.as_u64())
                .unwrap_or(0);

            let video_codec = container_details
                .video_codec
                .or_else(|| match extension.as_str() {
                    "mp4" | "m4v" => Some("h264/h265".to_string()),
                    "mov" => Some("prores/h264/hevc".to_string()),
                    "webm" => Some("vp8/vp9/av1".to_string()),
                    "mkv" => Some("matroska".to_string()),
                    _ => None,
                });

            let audio_codec = container_details
                .audio_codec
                .or_else(|| match extension.as_str() {
                    "mp4" | "mov" | "m4v" => Some("aac".to_string()),
                    "webm" => Some("opus/vorbis".to_string()),
                    _ => None,
                });

            video = Some(VideoMetadata {
                duration_ms,
                video_codec,
                audio_codec,
                frame_rate_fps: container_details.frame_rate_fps,
                bitrate_kbps: None,
                rotation_degrees: Some(rotation),
            });

            // 5. GPS Location if present
            if let Some(gps) = track.gps_info() {
                let lat_deg = gps.latitude.to_decimal_degrees();
                let lon_deg = gps.longitude.to_decimal_degrees();
                if let (Some(lat_val), Some(lon_val)) = (lat_deg, lon_deg) {
                    let lat = lat_val * gps.latitude_ref.sign();
                    let lon = lon_val * gps.longitude_ref.sign();
                    let alt = gps.altitude.meters();
                    location = GeoLocation::new(lat, lon, alt);
                }
            }
        } else if container_details.video_width.is_some()
            || container_details.rotation_degrees.is_some()
        {
            // nom-exif failed or unsupported, but atom scanner extracted basic container track info
            let mut width = container_details.video_width;
            let mut height = container_details.video_height;
            if (rotation == 90 || rotation == 270) && width.is_some() && height.is_some() {
                std::mem::swap(&mut width, &mut height);
            }
            if let (Some(w), Some(h)) = (width, height) {
                dimensions = Some(ImageDimensions {
                    width: w,
                    height: h,
                    orientation: Some(rotation as u16),
                });
            }
            video = Some(VideoMetadata {
                duration_ms: 0,
                video_codec: container_details.video_codec,
                audio_codec: container_details.audio_codec,
                frame_rate_fps: container_details.frame_rate_fps,
                bitrate_kbps: None,
                rotation_degrees: Some(rotation),
            });
        }

        // Fallback to filesystem timestamp if no container timestamp was discovered
        let final_temporal = match temporal {
            Some(t) => t,
            None => {
                warnings.push(
                    "No embedded container timestamp found; fell back to filesystem modified time"
                        .to_string(),
                );
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
            exposure: None,
            location,
            dimensions,
            video,
            fingerprint: None,
            extraction_status: ExtractionStatus {
                status: extraction_status_kind,
                warnings,
                error_message: None,
            },
            dedup: None,
        })
    }
}
