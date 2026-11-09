use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Semaphore};
use uuid::Uuid;

use crate::application::engine::ScanEngine;
use crate::application::metadata_engine::MetadataExtractionEngine;
use crate::application::progress_tracker::ProgressTracker;
use crate::domain::metadata_models::MediaMetadataDocument;
use crate::domain::ports::{
    BatchMetadataExtractor, DirectoryScanner, MetadataStorageSink, ProgressReporter, ProgressSink,
    ScanConfig,
};
use crate::domain::progress_models::{
    ExecutionSummary, ExtractionPerformanceProfile, OperationalPhase,
};

/// Orchestrates decoupled, high-throughput asynchronous media discovery, extraction, and persistence.
pub struct ExtractionPipeline {
    scan_engine: Arc<ScanEngine>,
    metadata_engine: Arc<MetadataExtractionEngine>,
    profile: ExtractionPerformanceProfile,
    storage_sink: Option<Arc<dyn MetadataStorageSink>>,
    progress_sink: Arc<dyn ProgressSink>,
}

impl ExtractionPipeline {
    pub fn new(
        scan_engine: Arc<ScanEngine>,
        metadata_engine: Arc<MetadataExtractionEngine>,
        profile: ExtractionPerformanceProfile,
        storage_sink: Option<Arc<dyn MetadataStorageSink>>,
        progress_sink: Arc<dyn ProgressSink>,
    ) -> Self {
        Self {
            scan_engine,
            metadata_engine,
            profile,
            storage_sink,
            progress_sink,
        }
    }

    /// Execute the full pipeline on the target root directory with graceful cancellation.
    pub async fn execute(
        &self,
        target_path: PathBuf,
        writer_opt: Option<Arc<tokio::sync::Mutex<Box<dyn tokio::io::AsyncWrite + Unpin + Send>>>>,
        cancel_token: Arc<AtomicBool>,
        is_pretty: bool,
    ) -> Result<ExecutionSummary, Box<dyn std::error::Error>> {
        let session_id = Uuid::new_v4();
        let tracker = Arc::new(ProgressTracker::with_options(
            session_id,
            target_path.clone(),
            self.profile.mode,
            self.profile.force_reindex,
        ));

        let scan_config = ScanConfig {
            root_paths: vec![target_path.clone()],
            batch_size: self.profile.batch_size,
            channel_capacity: self.profile.channel_capacity,
            skip_hidden: true,
            follow_symlinks: false,
            telemetry_interval: Duration::from_millis(self.profile.update_interval_ms),
        };

        tracker.set_phase(OperationalPhase::Discovery);
        let (_session, mut stream) = self.scan_engine.start_scan(scan_config).await?;

        // Live progress rendering loop on standard error
        let progress_sink = Arc::clone(&self.progress_sink);
        let tracker_for_render = Arc::clone(&tracker);
        let cancel_for_render = Arc::clone(&cancel_token);
        let update_interval = Duration::from_millis(self.profile.update_interval_ms.max(50));

        let progress_task = tokio::spawn(async move {
            while !cancel_for_render.load(Ordering::Relaxed) {
                let snap = tracker_for_render.snapshot();
                progress_sink.render_progress(&snap);
                if snap.active_phase == OperationalPhase::Completed {
                    break;
                }
                tokio::time::sleep(update_interval).await;
            }
        });

        // Error consumer from scanner
        let tracker_for_err = Arc::clone(&tracker);
        let error_task = tokio::spawn(async move {
            while let Some(_err) = stream.errors.recv().await {
                tracker_for_err.record_error();
            }
        });

        // Bounded worker pool concurrency control
        let concurrency = self.profile.worker_concurrency.max(1);
        let semaphore = Arc::new(Semaphore::new(concurrency));

        // Channel connecting parallel extraction workers to sequential/batch persistence
        let (doc_batch_tx, mut doc_batch_rx) = mpsc::channel::<(Vec<MediaMetadataDocument>, u64)>(
            self.profile.channel_capacity.max(16),
        );

        let engine = Arc::clone(&self.metadata_engine);
        let root = target_path.clone();
        let tracker_for_extract = Arc::clone(&tracker);
        let cancel_for_extract = Arc::clone(&cancel_token);

        let storage_sink_for_filter = self.storage_sink.clone();
        let force_reindex = self.profile.force_reindex;

        // Discovery & worker dispatch task
        let dispatch_task = tokio::spawn(async move {
            tracker_for_extract.set_phase(OperationalPhase::Extraction);

            while let Some(batch) = stream.batches.recv().await {
                if cancel_for_extract.load(Ordering::Relaxed) {
                    break;
                }

                let batch_bytes: u64 = batch.items.iter().map(|i| i.file_size_bytes).sum();
                tracker_for_extract.record_discovered(batch.count as u64, batch_bytes);

                let items_to_extract = if !force_reindex {
                    if let Some(ref sink) = storage_sink_for_filter {
                        match sink.filter_unmodified_items(&batch.items).await {
                            Ok(result) => {
                                if !result.unmodified_items.is_empty() {
                                    tracker_for_extract.record_skipped(
                                        result.unmodified_items.len() as u64,
                                        result.skipped_bytes,
                                    );
                                }
                                result.items_to_extract
                            }
                            Err(err) => {
                                tracing::warn!(
                                    error = %err,
                                    "Incremental filtering failed; proceeding with full extraction"
                                );
                                batch.items
                            }
                        }
                    } else {
                        batch.items
                    }
                } else {
                    batch.items
                };

                if items_to_extract.is_empty() {
                    continue;
                }

                let permit = match semaphore.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => break,
                };

                let eng = Arc::clone(&engine);
                let root_clone = root.clone();
                let tx = doc_batch_tx.clone();
                let tracker_w = Arc::clone(&tracker_for_extract);
                let extract_batch = crate::domain::models::DiscoveryBatch::new(
                    batch.batch_sequence,
                    batch.session_id,
                    items_to_extract,
                );

                tokio::spawn(async move {
                    let docs = eng.extract_batch(extract_batch, Some(&root_clone)).await;
                    let docs_bytes: u64 = docs.iter().map(|d| d.file_context.file_size_bytes).sum();
                    tracker_w.record_processed(docs.len() as u64, docs_bytes);

                    let _ = tx.send((docs, docs_bytes)).await;
                    drop(permit);
                });
            }

            tracker_for_extract.set_discovery_complete();
            drop(doc_batch_tx);
        });

        // Persistence & output consumer loop
        let storage_sink = self.storage_sink.clone();
        let tracker_for_persist = Arc::clone(&tracker);
        let writer_mutex = writer_opt.clone();

        while let Some((docs, _bytes)) = doc_batch_rx.recv().await {
            if cancel_token.load(Ordering::Relaxed) {
                break;
            }

            if storage_sink.is_some() {
                tracker_for_persist.set_phase(OperationalPhase::Persistence);
            }

            if let Some(ref sink) = storage_sink {
                match sink.save_batch(&docs).await {
                    Ok(stats) => {
                        tracker_for_persist.record_db_stats(
                            stats.upserted_count,
                            stats.modified_count,
                            stats.matched_count,
                            stats.failed_count,
                        );
                    }
                    Err(err) => {
                        tracker_for_persist.record_error();
                        tracing::error!(error = %err, "Failed to persist batch to storage sink");
                    }
                }
            }

            if let Some(ref writer_lock) = writer_mutex {
                let mut guard = writer_lock.lock().await;
                use tokio::io::AsyncWriteExt;
                for doc in &docs {
                    if is_pretty {
                        if let Ok(pretty) = serde_json::to_string_pretty(doc) {
                            let _ = guard.write_all(format!("{}\n", pretty).as_bytes()).await;
                        }
                    } else if let Ok(json) = serde_json::to_string(doc) {
                        let _ = guard.write_all(format!("{}\n", json).as_bytes()).await;
                    }
                }
                let _ = guard.flush().await;
            }
        }

        let _ = dispatch_task.await;
        let _ = error_task.await;

        tracker.set_phase(OperationalPhase::Completed);
        let cancelled = cancel_token.load(Ordering::Relaxed);
        let summary = tracker.finish(cancelled);

        // Render final snapshot and summary
        self.progress_sink.render_progress(&tracker.snapshot());
        let _ = progress_task.await;
        self.progress_sink.render_summary(&summary);

        Ok(summary)
    }
}
