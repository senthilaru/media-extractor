use chrono::Utc;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::RwLock;
use std::time::{Duration, Instant};
use uuid::Uuid;

use crate::domain::ports::ProgressReporter;
use crate::domain::progress_models::{
    ExecutionSummary, ExtractionMode, OperationalPhase, ScanProgressSnapshot,
};

struct ProgressSample {
    timestamp: Instant,
    items: u64,
    bytes: u64,
}

/// Thread-safe atomic progress tracker that calculates moving-average throughput and dynamic ETA.
pub struct ProgressTracker {
    pub session_id: Uuid,
    pub target_path: PathBuf,
    pub mode: ExtractionMode,
    start_time: Instant,
    started_at_utc: chrono::DateTime<Utc>,
    phase: RwLock<OperationalPhase>,
    directories_scanned: AtomicU64,
    items_discovered: AtomicU64,
    bytes_discovered: AtomicU64,
    items_processed: AtomicU64,
    bytes_processed: AtomicU64,
    items_skipped: AtomicU64,
    bytes_skipped: AtomicU64,
    items_persisted: AtomicU64,
    errors_encountered: AtomicU64,
    warnings_encountered: AtomicU64,
    db_upserted: AtomicU64,
    db_modified: AtomicU64,
    db_matched: AtomicU64,
    db_failed: AtomicU64,
    is_discovery_complete: AtomicBool,
    force_reindex: bool,
    samples: RwLock<VecDeque<ProgressSample>>,
}

impl ProgressTracker {
    /// Create a new progress tracker for the target session and path.
    pub fn new(session_id: Uuid, target_path: PathBuf, mode: ExtractionMode) -> Self {
        Self::with_options(session_id, target_path, mode, false)
    }

    /// Create a new progress tracker specifying whether force re-index is active.
    pub fn with_options(
        session_id: Uuid,
        target_path: PathBuf,
        mode: ExtractionMode,
        force_reindex: bool,
    ) -> Self {
        let now = Instant::now();
        let tracker = Self {
            session_id,
            target_path,
            mode,
            start_time: now,
            started_at_utc: Utc::now(),
            phase: RwLock::new(OperationalPhase::Discovery),
            directories_scanned: AtomicU64::new(0),
            items_discovered: AtomicU64::new(0),
            bytes_discovered: AtomicU64::new(0),
            items_processed: AtomicU64::new(0),
            bytes_processed: AtomicU64::new(0),
            items_skipped: AtomicU64::new(0),
            bytes_skipped: AtomicU64::new(0),
            items_persisted: AtomicU64::new(0),
            errors_encountered: AtomicU64::new(0),
            warnings_encountered: AtomicU64::new(0),
            db_upserted: AtomicU64::new(0),
            db_modified: AtomicU64::new(0),
            db_matched: AtomicU64::new(0),
            db_failed: AtomicU64::new(0),
            is_discovery_complete: AtomicBool::new(false),
            force_reindex,
            samples: RwLock::new(VecDeque::with_capacity(32)),
        };

        if let Ok(mut samples) = tracker.samples.write() {
            samples.push_back(ProgressSample {
                timestamp: now,
                items: 0,
                bytes: 0,
            });
        }

        tracker
    }

    /// Record directory visited during scanner traversal.
    pub fn record_directory_scanned(&self) {
        self.directories_scanned.fetch_add(1, Ordering::Relaxed);
    }

    /// Record database upsert/match/modify stats.
    pub fn record_db_stats(&self, upserted: u64, modified: u64, matched: u64, failed: u64) {
        self.db_upserted.fetch_add(upserted, Ordering::Relaxed);
        self.db_modified.fetch_add(modified, Ordering::Relaxed);
        self.db_matched.fetch_add(matched, Ordering::Relaxed);
        self.db_failed.fetch_add(failed, Ordering::Relaxed);
        self.items_persisted
            .fetch_add(upserted + matched, Ordering::Relaxed);
    }

    /// Calculate moving-average rates (items/sec and MB/sec).
    fn calculate_moving_rates(
        &self,
        now: Instant,
        total_items: u64,
        total_bytes: u64,
    ) -> (f64, f64) {
        if let Ok(mut samples) = self.samples.write() {
            // Retain samples within the last 10 seconds
            let window = Duration::from_secs(10);
            while samples.len() > 1
                && now.duration_since(samples.front().unwrap().timestamp) > window
            {
                samples.pop_front();
            }

            let oldest_timestamp = samples.front().unwrap().timestamp;
            let oldest_items = samples.front().unwrap().items;
            let oldest_bytes = samples.front().unwrap().bytes;
            let dt = now.duration_since(oldest_timestamp).as_secs_f64();

            // Record current point
            samples.push_back(ProgressSample {
                timestamp: now,
                items: total_items,
                bytes: total_bytes,
            });

            if dt >= 0.5 {
                let delta_items = total_items.saturating_sub(oldest_items) as f64;
                let delta_bytes = total_bytes.saturating_sub(oldest_bytes) as f64;
                let items_per_sec = (delta_items / dt).max(0.0);
                let mb_per_sec = ((delta_bytes / (1024.0 * 1024.0)) / dt).max(0.0);
                return (items_per_sec, mb_per_sec);
            }
        }

        // Fallback to overall rate since start if moving window is too small
        let elapsed = now.duration_since(self.start_time).as_secs_f64();
        if elapsed > 0.1 {
            let items_per_sec = (total_items as f64 / elapsed).max(0.0);
            let mb_per_sec = ((total_bytes as f64 / (1024.0 * 1024.0)) / elapsed).max(0.0);
            (items_per_sec, mb_per_sec)
        } else {
            (0.0, 0.0)
        }
    }
}

impl ProgressReporter for ProgressTracker {
    fn set_phase(&self, phase: OperationalPhase) {
        if let Ok(mut guard) = self.phase.write() {
            *guard = phase;
        }
    }

    fn record_discovered(&self, count: u64, bytes: u64) {
        self.items_discovered.fetch_add(count, Ordering::Relaxed);
        self.bytes_discovered.fetch_add(bytes, Ordering::Relaxed);
    }

    fn set_discovery_complete(&self) {
        self.is_discovery_complete.store(true, Ordering::Release);
    }

    fn record_processed(&self, count: u64, bytes: u64) {
        self.items_processed.fetch_add(count, Ordering::Relaxed);
        self.bytes_processed.fetch_add(bytes, Ordering::Relaxed);
    }

    fn record_skipped(&self, count: u64, bytes: u64) {
        self.items_skipped.fetch_add(count, Ordering::Relaxed);
        self.bytes_skipped.fetch_add(bytes, Ordering::Relaxed);
    }

    fn record_persisted(&self, count: u64) {
        self.items_persisted.fetch_add(count, Ordering::Relaxed);
    }

    fn record_error(&self) {
        self.errors_encountered.fetch_add(1, Ordering::Relaxed);
    }

    fn record_warning(&self) {
        self.warnings_encountered.fetch_add(1, Ordering::Relaxed);
    }

    fn snapshot(&self) -> ScanProgressSnapshot {
        let now = Instant::now();
        let elapsed_duration = now.duration_since(self.start_time);
        let active_phase = self
            .phase
            .read()
            .map(|p| *p)
            .unwrap_or(OperationalPhase::Discovery);

        let directories_scanned = self.directories_scanned.load(Ordering::Relaxed);
        let items_discovered = self.items_discovered.load(Ordering::Relaxed);
        let bytes_discovered = self.bytes_discovered.load(Ordering::Relaxed);
        let items_processed = self.items_processed.load(Ordering::Relaxed);
        let bytes_processed = self.bytes_processed.load(Ordering::Relaxed);
        let items_skipped = self.items_skipped.load(Ordering::Relaxed);
        let bytes_skipped = self.bytes_skipped.load(Ordering::Relaxed);
        let items_persisted = self.items_persisted.load(Ordering::Relaxed);
        let errors_encountered = self.errors_encountered.load(Ordering::Relaxed);
        let warnings_encountered = self.warnings_encountered.load(Ordering::Relaxed);
        let is_discovery_complete = self.is_discovery_complete.load(Ordering::Acquire);

        let completed_items = items_processed + items_skipped;
        let completed_bytes = bytes_processed + bytes_skipped;

        let (items_per_sec, mb_per_sec) =
            self.calculate_moving_rates(now, completed_items, completed_bytes);

        // Calculate moving-average ETA
        let completed_work = completed_items;
        let estimated_remaining = if items_per_sec > 0.05 && items_discovered > completed_work {
            let remaining_items = items_discovered.saturating_sub(completed_work);
            let secs = (remaining_items as f64 / items_per_sec).round();
            if secs.is_finite() && (0.0..=864_000.0).contains(&secs) {
                Some(Duration::from_secs(secs as u64))
            } else {
                None
            }
        } else {
            None
        };

        ScanProgressSnapshot {
            session_id: self.session_id,
            elapsed_duration,
            active_phase,
            directories_scanned,
            items_discovered,
            bytes_discovered,
            items_processed,
            bytes_processed,
            items_skipped,
            bytes_skipped,
            items_persisted,
            items_per_sec,
            mb_per_sec,
            estimated_remaining,
            errors_encountered,
            warnings_encountered,
            is_discovery_complete,
        }
    }

    fn finish(&self, cancelled: bool) -> ExecutionSummary {
        let now = Instant::now();
        let elapsed_duration = now.duration_since(self.start_time);
        let completed_at = Utc::now();

        let total_discovered = self.items_discovered.load(Ordering::Relaxed);
        let total_skipped = self.items_skipped.load(Ordering::Relaxed);
        let total_processed = self.items_processed.load(Ordering::Relaxed);
        let total_bytes_processed = self.bytes_processed.load(Ordering::Relaxed);
        let total_bytes_skipped = self.bytes_skipped.load(Ordering::Relaxed);
        let db_upserted = self.db_upserted.load(Ordering::Relaxed);
        let db_modified = self.db_modified.load(Ordering::Relaxed);
        let db_matched = self.db_matched.load(Ordering::Relaxed);
        let db_failed = self.db_failed.load(Ordering::Relaxed);
        let extraction_errors = self.errors_encountered.load(Ordering::Relaxed);
        let warnings_count = self.warnings_encountered.load(Ordering::Relaxed);

        let elapsed_secs = elapsed_duration.as_secs_f64().max(0.001);
        let average_items_per_sec = (total_processed + total_skipped) as f64 / elapsed_secs;
        let average_mb_per_sec = ((total_bytes_processed + total_bytes_skipped) as f64
            / (1024.0 * 1024.0))
            / elapsed_secs;

        ExecutionSummary {
            session_id: self.session_id,
            target_path: self.target_path.clone(),
            mode: self.mode,
            started_at: self.started_at_utc,
            completed_at,
            elapsed_duration,
            total_discovered,
            total_skipped,
            total_processed,
            total_bytes_processed,
            total_bytes_skipped,
            average_items_per_sec,
            average_mb_per_sec,
            db_upserted,
            db_matched,
            db_modified,
            db_failed,
            extraction_errors,
            warnings_count,
            was_cancelled: cancelled,
            force_reindex: self.force_reindex,
        }
    }
}
