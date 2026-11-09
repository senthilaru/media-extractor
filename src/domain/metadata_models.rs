use chrono::{DateTime, Datelike, NaiveDateTime, Timelike, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use uuid::Uuid;

use crate::domain::dedup_models::MediaFingerprint;
use crate::domain::models::MediaCategory;

/// Normalized document representing a processed media asset for MongoDB or NDJSON export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaMetadataDocument {
    /// Unique document identifier (UUID v4)
    pub id: Uuid,

    /// Filesystem and hierarchy context
    pub file_context: FileContext,

    /// Normalized capture timestamp and calendar facets
    pub temporal: TemporalMetadata,

    /// Hardware specifications (camera body, lens, software)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub camera: Option<CameraMetadata>,

    /// Photographic capture settings (aperture, shutter speed, ISO, focal length)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exposure: Option<ExposureMetadata>,

    /// Geolocation coordinates in decimal degrees
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<GeoLocation>,

    /// Pixel dimensions and orientation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dimensions: Option<ImageDimensions>,

    /// Video container metadata (duration, codecs, fps, rotation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video: Option<VideoMetadata>,

    /// Cryptographic and perceptual visual content fingerprints for deduplication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<MediaFingerprint>,

    /// Operational status and diagnostic warnings/errors
    pub extraction_status: ExtractionStatus,
}

/// Filesystem and spatial path context relative to the scan root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileContext {
    /// The root folder supplied to the scanner (e.g. `/Volumes/SP-1`)
    pub scanned_root: PathBuf,

    /// Full canonical or absolute path on disk
    pub file_path: PathBuf,

    /// Path relative to `scanned_root`
    pub relative_path: PathBuf,

    /// Base file name with extension (e.g. `IMG_1042.CR3`)
    pub file_name: String,

    /// Byte size reported by filesystem (>= 0)
    pub file_size_bytes: u64,

    /// Normalized lowercase extension without leading dot
    pub extension: String,

    /// Media category (`Photo`, `Video`, `RawPhoto`)
    pub media_category: MediaCategory,

    /// Filesystem modification timestamp (mtime) in UTC
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modified_at: Option<DateTime<Utc>>,
}

/// Result of evaluating a batch of discovered items against existing database records.
#[derive(Debug, Clone, Default)]
pub struct FilterEvaluationResult {
    /// Discovered items verified as already present in database with matching size and timestamp
    pub unmodified_items: Vec<crate::domain::models::DiscoveredMediaItem>,
    /// Items that are new or modified and must be extracted
    pub items_to_extract: Vec<crate::domain::models::DiscoveredMediaItem>,
    /// Total byte size of unmodified items
    pub skipped_bytes: u64,
    /// Total byte size of items to extract
    pub extract_bytes: u64,
}

/// Capture datetime decomposed into integer calendar facets for MongoDB querying.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemporalMetadata {
    /// UTC capture instant
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp_utc: Option<DateTime<Utc>>,

    /// ISO 8601 formatted timestamp string in UTC
    pub iso_timestamp: String,

    /// Integer year facet for database partitioning / indexing (e.g. 2026)
    pub year: i32,

    /// Integer month facet (1..=12)
    pub month: u32,

    /// Integer day facet (1..=31)
    pub day: u32,

    /// Integer hour facet (0..=23)
    pub hour: u32,

    /// Integer minute facet (0..=59)
    pub minute: u32,

    /// Integer second facet (0..=59)
    pub second: u32,

    /// Origin source of the timestamp
    pub source: TimestampSource,
}

impl TemporalMetadata {
    /// Constructs `TemporalMetadata` from a `DateTime<Utc>`.
    pub fn from_utc_datetime(dt: DateTime<Utc>, source: TimestampSource) -> Self {
        Self {
            timestamp_utc: Some(dt),
            iso_timestamp: dt.to_rfc3339(),
            year: dt.year(),
            month: dt.month(),
            day: dt.day(),
            hour: dt.hour(),
            minute: dt.minute(),
            second: dt.second(),
            source,
        }
    }

    /// Constructs `TemporalMetadata` from filesystem `SystemTime` as fallback.
    pub fn from_system_time(st: SystemTime) -> Self {
        let dt: DateTime<Utc> = st.into();
        Self::from_utc_datetime(dt, TimestampSource::FileSystemModified)
    }

    /// Parses various camera EXIF datetime string formats into `TemporalMetadata`.
    /// Handles `YYYY:MM:DD HH:MM:SS`, `YYYY-MM-DD HH:MM:SS`, and RFC 3339 / ISO 8601.
    pub fn parse_exif_date(s: &str, source: TimestampSource) -> Option<Self> {
        let s = s.trim();
        if s.is_empty() {
            return None;
        }

        // Try RFC 3339 / ISO 8601 first (e.g. "2026-10-14T08:12:45Z")
        if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
            return Some(Self::from_utc_datetime(dt.with_timezone(&Utc), source));
        }

        // Standard EXIF format: "YYYY:MM:DD HH:MM:SS"
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y:%m:%d %H:%M:%S") {
            let dt = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
            return Some(Self::from_utc_datetime(dt, source));
        }

        // Common alternative: "YYYY-MM-DD HH:MM:SS"
        if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
            let dt = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
            return Some(Self::from_utc_datetime(dt, source));
        }

        // Date only: "YYYY:MM:DD"
        if let Ok(naive_date) = chrono::NaiveDate::parse_from_str(s, "%Y:%m:%d") {
            if let Some(naive) = naive_date.and_hms_opt(0, 0, 0) {
                let dt = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
                return Some(Self::from_utc_datetime(dt, source));
            }
        }

        // Alternative date only: "YYYY-MM-DD"
        if let Ok(naive_date) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
            if let Some(naive) = naive_date.and_hms_opt(0, 0, 0) {
                let dt = DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc);
                return Some(Self::from_utc_datetime(dt, source));
            }
        }

        None
    }
}

/// Provenance tag indicating where the primary timestamp originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TimestampSource {
    ExifDateTimeOriginal,
    ExifCreateDate,
    QuickTimeCreationTime,
    FileSystemModified,
}

/// Hardware specifics of the capture device.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct CameraMetadata {
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens_model: Option<String>,
    pub software: Option<String>,
}

/// Photographic capture settings.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ExposureMetadata {
    pub focal_length_mm: Option<f64>,
    pub focal_length_35mm: Option<f64>,
    pub f_number: Option<f64>,
    pub exposure_time_seconds: Option<f64>,
    pub exposure_time_str: Option<String>,
    pub iso: Option<u32>,
    pub flash_fired: Option<bool>,
}

/// Geolocation coordinates in decimal degrees with bounds [-90, 90] and [-180, 180].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GeoLocation {
    pub latitude: f64,
    pub longitude: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub altitude_meters: Option<f64>,
}

impl GeoLocation {
    /// Constructs and validates a `GeoLocation`, ensuring coordinates lie within valid ranges.
    pub fn new(latitude: f64, longitude: f64, altitude_meters: Option<f64>) -> Option<Self> {
        if !(-90.0..=90.0).contains(&latitude) || !(-180.0..=180.0).contains(&longitude) {
            return None;
        }
        // Exclude common null coordinates (0.0, 0.0) if uncalibrated
        if latitude.abs() < f64::EPSILON && longitude.abs() < f64::EPSILON {
            return None;
        }
        Some(Self {
            latitude,
            longitude,
            altitude_meters,
        })
    }
}

/// Pixel dimensions and EXIF orientation tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ImageDimensions {
    pub width: u32,
    pub height: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub orientation: Option<u16>,
}

/// Video container specific metadata.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct VideoMetadata {
    pub duration_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_codec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_codec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_rate_fps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate_kbps: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation_degrees: Option<i32>,
}

/// Status and operational diagnostics of the extraction process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtractionStatus {
    pub status: ExtractionResultKind,
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
}

impl Default for ExtractionStatus {
    fn default() -> Self {
        Self {
            status: ExtractionResultKind::Success,
            warnings: Vec::new(),
            error_message: None,
        }
    }
}

/// Result category for extraction fidelity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtractionResultKind {
    Success,
    Partial,
    Failed,
}

/// Live thread-safe atomic counters tracking metadata extraction outcomes.
#[derive(Debug, Default)]
pub struct AtomicExtractionCounters {
    pub total_processed: AtomicU64,
    pub success_count: AtomicU64,
    pub partial_count: AtomicU64,
    pub failed_count: AtomicU64,
}

impl AtomicExtractionCounters {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, kind: ExtractionResultKind) {
        self.total_processed.fetch_add(1, Ordering::Relaxed);
        match kind {
            ExtractionResultKind::Success => {
                self.success_count.fetch_add(1, Ordering::Relaxed);
            }
            ExtractionResultKind::Partial => {
                self.partial_count.fetch_add(1, Ordering::Relaxed);
            }
            ExtractionResultKind::Failed => {
                self.failed_count.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    pub fn snapshot(&self, elapsed: Duration) -> ExtractionMetrics {
        let total = self.total_processed.load(Ordering::Relaxed);
        let success = self.success_count.load(Ordering::Relaxed);
        let partial = self.partial_count.load(Ordering::Relaxed);
        let failed = self.failed_count.load(Ordering::Relaxed);
        let elapsed_secs = elapsed.as_secs_f64();
        let throughput_fps = if elapsed_secs > 0.0 {
            total as f64 / elapsed_secs
        } else {
            0.0
        };

        ExtractionMetrics {
            total_processed: total,
            success_count: success,
            partial_count: partial,
            failed_count: failed,
            elapsed_duration: elapsed,
            throughput_fps,
        }
    }
}

/// Thread-safe reference to atomic extraction counters.
pub type SharedExtractionCounters = Arc<AtomicExtractionCounters>;

/// Point-in-time snapshot of extraction telemetry metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractionMetrics {
    pub total_processed: u64,
    pub success_count: u64,
    pub partial_count: u64,
    pub failed_count: u64,
    pub elapsed_duration: Duration,
    pub throughput_fps: f64,
}

/// Operational runtime profile for configuring defaults and security enforcement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, clap::ValueEnum, Serialize, Deserialize)]
pub enum AppProfile {
    #[default]
    Local,
    Production,
}

impl std::fmt::Display for AppProfile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local => write!(f, "local"),
            Self::Production => write!(f, "production"),
        }
    }
}

impl std::str::FromStr for AppProfile {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_lowercase().as_str() {
            "local" | "dev" | "development" => Ok(Self::Local),
            "production" | "prod" => Ok(Self::Production),
            other => Err(format!(
                "Unknown profile '{}'. Valid profiles are 'local' or 'production'.",
                other
            )),
        }
    }
}

/// Configuration options for persisting metadata directly to a database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DatabaseSinkConfig {
    /// Full database connection string (e.g. mongodb://user:pass@host:port/db?authSource=db).
    pub connection_uri: String,

    /// Target database name (parsed from URI or default).
    pub database_name: String,

    /// Target collection for media metadata documents (default: "media_assets").
    pub collection_name: String,

    /// Maximum number of documents per bulk write operation (default: 500).
    pub batch_size: usize,

    /// Connection and socket timeout in seconds (default: 10).
    pub timeout_seconds: u64,

    /// Maximum retries for transient connection/write timeouts (default: 3).
    pub max_retries: u32,
}

impl DatabaseSinkConfig {
    pub const DEFAULT_COLLECTION: &'static str = "media_assets";
    pub const DEFAULT_BATCH_SIZE: usize = 500;
    pub const DEFAULT_TIMEOUT_SECONDS: u64 = 10;
    pub const DEFAULT_MAX_RETRIES: u32 = 3;

    pub fn new(connection_uri: impl Into<String>, collection_name: Option<String>) -> Self {
        let uri = connection_uri.into();
        let database_name =
            Self::parse_database_name(&uri).unwrap_or_else(|| "media_index_db".to_string());
        let collection = collection_name
            .filter(|c| !c.trim().is_empty())
            .unwrap_or_else(|| Self::DEFAULT_COLLECTION.to_string());

        Self {
            connection_uri: uri,
            database_name,
            collection_name: collection,
            batch_size: Self::DEFAULT_BATCH_SIZE,
            timeout_seconds: Self::DEFAULT_TIMEOUT_SECONDS,
            max_retries: Self::DEFAULT_MAX_RETRIES,
        }
    }

    /// Helper to parse database name from mongodb URI path segment.
    pub fn parse_database_name(uri: &str) -> Option<String> {
        let path_part = uri.split('?').next()?;
        let after_scheme = path_part.split("://").nth(1)?;
        let slash_pos = after_scheme.find('/')?;
        let db_part = &after_scheme[slash_pos + 1..];
        let db_name = db_part.trim_matches('/');
        if !db_name.is_empty() {
            Some(db_name.to_string())
        } else {
            None
        }
    }

    /// Percent-encodes characters for safe inclusion in URI credentials per RFC 3986.
    pub fn url_encode_component(input: &str) -> String {
        let mut encoded = String::with_capacity(input.len());
        for b in input.bytes() {
            match b {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    encoded.push(b as char);
                }
                _ => {
                    use std::fmt::Write;
                    let _ = write!(encoded, "%{:02X}", b);
                }
            }
        }
        encoded
    }

    /// Interpolates an externalized password into a connection URI, replacing placeholders
    /// or injecting credentials into an unauthenticated username specification.
    pub fn interpolate_secret(uri: &str, password: Option<&str>) -> String {
        let mut resolved = uri.to_string();

        if let Some(pass) = password {
            let encoded_pass = Self::url_encode_component(pass);

            // Replace known placeholders
            if resolved.contains("<MONGODB_PASSWORD>") {
                resolved = resolved.replace("<MONGODB_PASSWORD>", &encoded_pass);
            }
            if resolved.contains("<password>") {
                resolved = resolved.replace("<password>", &encoded_pass);
            }
            if resolved.contains("${MONGODB_PASSWORD}") {
                resolved = resolved.replace("${MONGODB_PASSWORD}", &encoded_pass);
            }

            // If URI is in format mongodb://username@host and lacks a password separator:
            if let Some((scheme, rest)) = resolved.split_once("://") {
                if let Some((user_info, host_part)) = rest.split_once('@') {
                    if !user_info.contains(':') && !user_info.is_empty() {
                        resolved =
                            format!("{}://{}:{}@{}", scheme, user_info, encoded_pass, host_part);
                    }
                }
            }
        }

        resolved
    }

    /// Validates configuration against the operational profile rules.
    pub fn validate_for_profile(
        &self,
        profile: AppProfile,
    ) -> Result<(), crate::domain::errors::StorageError> {
        use crate::domain::errors::StorageError;

        if profile == AppProfile::Production {
            if self.connection_uri.contains("<MONGODB_PASSWORD>")
                || self.connection_uri.contains("<password>")
                || self.connection_uri.contains("${MONGODB_PASSWORD}")
            {
                return Err(StorageError::ConfigurationError(
                    "Production profile: unresolved password placeholder in MongoDB URI. Please set MONGODB_PASSWORD environment variable or provide authenticated MONGODB_URI.".into(),
                ));
            }

            if let Some((_, rest)) = self.connection_uri.split_once("://") {
                if let Some((user_info, _)) = rest.split_once('@') {
                    if let Some((user, pass)) = user_info.split_once(':') {
                        if user.trim().is_empty() || pass.trim().is_empty() {
                            return Err(StorageError::ConfigurationError(
                                "Production profile requires explicit username and password."
                                    .into(),
                            ));
                        }
                        if pass == "password" || pass == "admin" || pass == "root" {
                            return Err(StorageError::ConfigurationError(
                                "Production profile rejects default/insecure passwords ('password', 'admin', 'root').".into(),
                            ));
                        }
                        return Ok(());
                    }
                }
            }

            return Err(StorageError::ConfigurationError(
                "Production profile requires authenticated MongoDB connection URI with username and password.".into(),
            ));
        }

        Ok(())
    }

    /// Produces a sanitized version of the URI with passwords replaced with `***`.
    pub fn sanitized_uri(&self) -> String {
        Self::sanitize_uri_str(&self.connection_uri)
    }

    /// Static utility to sanitize any connection URI string.
    pub fn sanitize_uri_str(uri: &str) -> String {
        if let Some((prefix, rest)) = uri.split_once("://") {
            if let Some((user_info, host_rest)) = rest.split_once('@') {
                if let Some((user, _pass)) = user_info.split_once(':') {
                    return format!("{}://{}:***@{}", prefix, user, host_rest);
                }
            }
        }
        uri.to_string()
    }
}
