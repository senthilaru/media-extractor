use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

use crate::application::batch_accumulator::BatchAccumulator;
use crate::domain::errors::{ScanError, ScanErrorKind};
use crate::domain::models::{
    AtomicSessionCounters, DiscoveredMediaItem, ScanErrorRecord, ScanSession,
};
use crate::domain::ports::{DirectoryScanner, MediaClassifier, ScanConfig, ScanStream};
use crate::infrastructure::classifier::PhfMediaClassifier;
use crate::infrastructure::cycle_detector::CycleDetector;
use crate::infrastructure::telemetry::HeartbeatReporter;
use crate::infrastructure::walker::{ParallelWalker, WalkItem};

/// High-performance scanning engine orchestrating directory traversal, batching, and backpressure.
pub struct ScanEngine {
    classifier: Arc<dyn MediaClassifier>,
    active_sessions: Arc<Mutex<HashMap<Uuid, Arc<AtomicBool>>>>,
}

impl Default for ScanEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ScanEngine {
    pub fn new() -> Self {
        Self {
            classifier: Arc::new(PhfMediaClassifier::new()),
            active_sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_classifier(classifier: Arc<dyn MediaClassifier>) -> Self {
        Self {
            classifier,
            active_sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait::async_trait]
impl DirectoryScanner for ScanEngine {
    async fn start_scan(&self, config: ScanConfig) -> Result<(ScanSession, ScanStream), ScanError> {
        if config.root_paths.is_empty() {
            return Err(ScanError::InvalidPath("No root paths provided".into()));
        }

        for path in &config.root_paths {
            if !path.exists() {
                return Err(ScanError::InvalidPath(path.clone()));
            }
        }

        let mut session = ScanSession::new(config.root_paths.clone());
        session.status = crate::domain::models::SessionStatus::Running;
        let session_id = session.session_id;

        let cancel_flag = Arc::new(AtomicBool::new(false));
        {
            let mut sessions = self.active_sessions.lock().await;
            sessions.insert(session_id, Arc::clone(&cancel_flag));
        }

        let (batch_tx, batch_rx) = mpsc::channel(config.channel_capacity);
        let (error_tx, error_rx) = mpsc::channel(config.channel_capacity.max(16) * 2);
        let (telemetry_tx, telemetry_rx) = mpsc::channel(16);

        let counters = Arc::new(AtomicSessionCounters::default());
        let in_flight_batches = Arc::new(AtomicUsize::new(0));

        // Spawn heartbeat telemetry reporter
        let reporter = HeartbeatReporter::new(
            session_id,
            Arc::clone(&counters),
            session.started_at,
            config.telemetry_interval,
            telemetry_tx,
        );

        let in_flight_for_telemetry = Arc::clone(&in_flight_batches);
        let reporter_handle =
            reporter.spawn(move || in_flight_for_telemetry.load(Ordering::Relaxed));

        // Spawn crawler task
        let classifier = Arc::clone(&self.classifier);
        let active_sessions = Arc::clone(&self.active_sessions);
        let roots = config.root_paths.clone();
        let skip_hidden = config.skip_hidden;
        let follow_symlinks = config.follow_symlinks;
        let batch_size = config.batch_size;

        tokio::spawn(async move {
            let mut accumulator = BatchAccumulator::new(session_id, batch_size, batch_tx);
            let mut cycle_detector = CycleDetector::new();

            'outer: for root in roots {
                if cancel_flag.load(Ordering::Relaxed) {
                    break;
                }

                if follow_symlinks {
                    if let Ok(visited) = cycle_detector.is_or_record_visited(&root) {
                        if visited {
                            counters.errors_encountered.fetch_add(1, Ordering::Relaxed);
                            let rec = ScanErrorRecord::new(
                                root.clone(),
                                ScanErrorKind::CircularSymlink,
                                "Root path is a circular symlink loop",
                            );
                            let _ = error_tx.send(rec).await;
                            continue;
                        }
                    }
                }

                let walk_items = ParallelWalker::walk(&root, skip_hidden, follow_symlinks);

                for item in walk_items {
                    if cancel_flag.load(Ordering::Relaxed) {
                        break 'outer;
                    }

                    match item {
                        WalkItem::Directory { path } => {
                            counters.directories_scanned.fetch_add(1, Ordering::Relaxed);
                            if follow_symlinks && path.is_symlink() {
                                if let Ok(is_cycle) = cycle_detector.is_or_record_visited(&path) {
                                    if is_cycle {
                                        counters.errors_encountered.fetch_add(1, Ordering::Relaxed);
                                        let rec = ScanErrorRecord::new(
                                            path,
                                            ScanErrorKind::CircularSymlink,
                                            "Circular directory symlink detected",
                                        );
                                        let _ = error_tx.send(rec).await;
                                        continue;
                                    }
                                }
                            }
                        }
                        WalkItem::File {
                            path,
                            file_size,
                            modified_at,
                        } => {
                            counters.files_inspected.fetch_add(1, Ordering::Relaxed);

                            if let Some(category) = classifier.classify(&path) {
                                let ext = path
                                    .extension()
                                    .and_then(|e| e.to_str())
                                    .map(|s| s.to_ascii_lowercase())
                                    .unwrap_or_default();

                                counters
                                    .media_files_discovered
                                    .fetch_add(1, Ordering::Relaxed);

                                let media_item = DiscoveredMediaItem::new(
                                    path,
                                    file_size,
                                    modified_at,
                                    category,
                                    ext,
                                );

                                in_flight_batches.fetch_add(1, Ordering::Relaxed);
                                if accumulator.push(media_item).await.is_err() {
                                    // Consumer dropped receiver
                                    break 'outer;
                                }
                            }
                        }
                        WalkItem::Error {
                            path,
                            kind,
                            message,
                        } => {
                            counters.errors_encountered.fetch_add(1, Ordering::Relaxed);
                            let record = ScanErrorRecord::new(path, kind, message);
                            let _ = error_tx.send(record).await;
                        }
                    }
                }
            }

            // Flush remaining items if not cancelled
            if !cancel_flag.load(Ordering::Relaxed) {
                let _ = accumulator.flush().await;
            }

            // Explicitly drop accumulator (which drops the batch sender)
            drop(accumulator);

            // Abort background telemetry reporter
            reporter_handle.abort();

            // Cleanup session tracking
            let mut sessions = active_sessions.lock().await;
            sessions.remove(&session_id);
        });

        let stream = ScanStream {
            batches: batch_rx,
            errors: error_rx,
            telemetry: telemetry_rx,
        };

        Ok((session, stream))
    }

    async fn cancel_scan(&self, session_id: &Uuid) -> Result<(), ScanError> {
        let sessions = self.active_sessions.lock().await;
        if let Some(flag) = sessions.get(session_id) {
            flag.store(true, Ordering::Relaxed);
            Ok(())
        } else {
            Err(ScanError::SessionNotFound(*session_id))
        }
    }
}
