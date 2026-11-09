use chrono::{DateTime, Datelike, Timelike, Utc};
use std::time::SystemTime;

use metadata_extractor::domain::metadata_models::{TemporalMetadata, TimestampSource};

#[test]
fn test_parse_standard_exif_date() {
    let date_str = "2026:10:14 08:12:45";
    let temporal =
        TemporalMetadata::parse_exif_date(date_str, TimestampSource::ExifDateTimeOriginal)
            .expect("Standard EXIF date format should parse");

    assert_eq!(temporal.year, 2026);
    assert_eq!(temporal.month, 10);
    assert_eq!(temporal.day, 14);
    assert_eq!(temporal.hour, 8);
    assert_eq!(temporal.minute, 12);
    assert_eq!(temporal.second, 45);
    assert_eq!(temporal.source, TimestampSource::ExifDateTimeOriginal);
    assert_eq!(temporal.iso_timestamp, "2026-10-14T08:12:45+00:00");
}

#[test]
fn test_parse_iso8601_rfc3339_date() {
    let date_str = "2026-05-20T14:30:00Z";
    let temporal = TemporalMetadata::parse_exif_date(date_str, TimestampSource::ExifCreateDate)
        .expect("RFC3339 date should parse");

    assert_eq!(temporal.year, 2026);
    assert_eq!(temporal.month, 5);
    assert_eq!(temporal.day, 20);
    assert_eq!(temporal.hour, 14);
    assert_eq!(temporal.minute, 30);
    assert_eq!(temporal.second, 0);
    assert_eq!(temporal.source, TimestampSource::ExifCreateDate);
}

#[test]
fn test_parse_dash_separated_date() {
    let date_str = "2025-12-31 23:59:59";
    let temporal =
        TemporalMetadata::parse_exif_date(date_str, TimestampSource::ExifDateTimeOriginal)
            .expect("Dash-separated datetime should parse");

    assert_eq!(temporal.year, 2025);
    assert_eq!(temporal.month, 12);
    assert_eq!(temporal.day, 31);
    assert_eq!(temporal.hour, 23);
    assert_eq!(temporal.minute, 59);
    assert_eq!(temporal.second, 59);
}

#[test]
fn test_parse_date_only() {
    let date_str = "2024:02:29";
    let temporal =
        TemporalMetadata::parse_exif_date(date_str, TimestampSource::ExifDateTimeOriginal)
            .expect("Date-only string should parse with midnight default");

    assert_eq!(temporal.year, 2024);
    assert_eq!(temporal.month, 2);
    assert_eq!(temporal.day, 29);
    assert_eq!(temporal.hour, 0);
    assert_eq!(temporal.minute, 0);
    assert_eq!(temporal.second, 0);
}

#[test]
fn test_parse_empty_or_invalid() {
    assert!(TemporalMetadata::parse_exif_date("", TimestampSource::ExifDateTimeOriginal).is_none());
    assert!(TemporalMetadata::parse_exif_date(
        "invalid-date-string",
        TimestampSource::ExifDateTimeOriginal
    )
    .is_none());
    assert!(
        TemporalMetadata::parse_exif_date("   ", TimestampSource::ExifDateTimeOriginal).is_none()
    );
}

#[test]
fn test_filesystem_fallback_temporal() {
    let now = SystemTime::now();
    let dt: DateTime<Utc> = now.into();
    let temporal = TemporalMetadata::from_system_time(now);

    assert_eq!(temporal.source, TimestampSource::FileSystemModified);
    assert_eq!(temporal.year, dt.year());
    assert_eq!(temporal.month, dt.month());
    assert_eq!(temporal.day, dt.day());
    assert_eq!(temporal.hour, dt.hour());
    assert_eq!(temporal.minute, dt.minute());
}
