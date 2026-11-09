use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use uuid::Uuid;

use crate::domain::errors::ScanErrorKind;

/// High-level categorization of discovered media.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MediaCategory {
    Photo,
    Video,
    RawPhoto,
}

impl std::fmt::Display for MediaCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Photo => write!(f, "Photo"),
            Self::Video => write!(f, "Video"),
            Self::RawPhoto => write!(f, "RawPhoto"),
        }
    }
}

/// An immutable value object representing an individual media file identified on disk.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DiscoveredMediaItem {
    pub path: PathBuf,
    pub file_size_bytes: u64,
    pub modified_at: Option<SystemTime>,
    pub category: MediaCategory,
    pub extension: String,
}

impl DiscoveredMediaItem {
    pub fn new(
        path: PathBuf,
        file_size_bytes: u64,
        modified_at: Option<SystemTime>,
        category: MediaCategory,
        extension: impl Into<String>,
    ) -> Self {
        Self {
            path,
            file_size_bytes,
            modified_at,
            category,
            extension: extension.into(),
        }
    }
}

/// An ordered batch of discovered items transmitted through asynchronous channels.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DiscoveryBatch {
    pub batch_sequence: u64,
    pub session_id: Uuid,
    pub items: Vec<DiscoveredMediaItem>,
    pub count: usize,
    pub created_at: SystemTime,
}

impl DiscoveryBatch {
    pub fn new(batch_sequence: u64, session_id: Uuid, items: Vec<DiscoveredMediaItem>) -> Self {
        let count = items.len();
        Self {
            batch_sequence,
            session_id,
            items,
            count,
            created_at: SystemTime::now(),
        }
    }
}

/// Operational state of a scan session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SessionStatus {
    Initializing,
    Running,
    Paused,
    Completed,
    Cancelled,
    Failed,
}

/// Live atomic performance counters used during traversal.
#[derive(Debug, Default)]
pub struct AtomicSessionCounters {
    pub directories_scanned: AtomicU64,
    pub files_inspected: AtomicU64,
    pub media_files_discovered: AtomicU64,
    pub errors_encountered: AtomicU64,
}

/// Thread-safe reference to atomic counters.
pub type SharedSessionCounters = Arc<AtomicSessionCounters>;

/// Point-in-time snapshot of traversal metrics.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SessionMetrics {
    pub session_id: Uuid,
    pub directories_scanned: u64,
    pub files_inspected: u64,
    pub media_files_discovered: u64,
    pub errors_encountered: u64,
    pub elapsed_duration: Duration,
    pub items_per_sec: f64,
    pub queue_depth: usize,
}

impl SessionMetrics {
    pub fn from_counters(
        session_id: Uuid,
        counters: &AtomicSessionCounters,
        started_at: SystemTime,
        queue_depth: usize,
    ) -> Self {
        let elapsed = SystemTime::now()
            .duration_since(started_at)
            .unwrap_or(Duration::from_millis(1));
        let inspected = counters.files_inspected.load(Ordering::Relaxed);
        let elapsed_secs = elapsed.as_secs_f64();
        let items_per_sec = if elapsed_secs > 0.0 {
            inspected as f64 / elapsed_secs
        } else {
            0.0
        };

        Self {
            session_id,
            directories_scanned: counters.directories_scanned.load(Ordering::Relaxed),
            files_inspected: inspected,
            media_files_discovered: counters.media_files_discovered.load(Ordering::Relaxed),
            errors_encountered: counters.errors_encountered.load(Ordering::Relaxed),
            elapsed_duration: elapsed,
            items_per_sec,
            queue_depth,
        }
    }
}

/// Diagnostic entry capturing non-fatal traversal failures for auditing.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScanErrorRecord {
    pub path: PathBuf,
    pub kind: ScanErrorKind,
    pub message: String,
    pub occurred_at: SystemTime,
}

impl ScanErrorRecord {
    pub fn new(path: PathBuf, kind: ScanErrorKind, message: impl Into<String>) -> Self {
        Self {
            path,
            kind,
            message: message.into(),
            occurred_at: SystemTime::now(),
        }
    }
}

/// Aggregate root representing an active or completed directory traversal execution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ScanSession {
    pub session_id: Uuid,
    pub root_paths: Vec<PathBuf>,
    pub started_at: SystemTime,
    pub completed_at: Option<SystemTime>,
    pub status: SessionStatus,
}

impl ScanSession {
    pub fn new(root_paths: Vec<PathBuf>) -> Self {
        Self {
            session_id: Uuid::new_v4(),
            root_paths,
            started_at: SystemTime::now(),
            completed_at: None,
            status: SessionStatus::Initializing,
        }
    }

    pub fn complete(&mut self) {
        self.status = SessionStatus::Completed;
        self.completed_at = Some(SystemTime::now());
    }

    pub fn cancel(&mut self) {
        self.status = SessionStatus::Cancelled;
        self.completed_at = Some(SystemTime::now());
    }

    pub fn fail(&mut self) {
        self.status = SessionStatus::Failed;
        self.completed_at = Some(SystemTime::now());
    }
}
