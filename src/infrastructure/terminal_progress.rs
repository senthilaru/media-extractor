use std::io::IsTerminal;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use crate::domain::ports::ProgressSink;
use crate::domain::progress_models::{ExecutionSummary, ScanProgressSnapshot};

/// Helper to format raw bytes into human-readable representation.
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Helper to format a duration into standard human-readable format (e.g. "3m 42s" or "45s").
pub fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs();
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let rem_secs = secs % 60;

    if hours > 0 {
        format!("{}h {:02}m {:02}s", hours, mins, rem_secs)
    } else if mins > 0 {
        format!("{}m {:02}s", mins, rem_secs)
    } else {
        format!("{}s", rem_secs)
    }
}

/// Terminal progress sink that adapts between interactive TTYs, non-interactive log lines, and JSON telemetry.
pub struct IndicatifTerminalProgress {
    is_tty: bool,
    json_telemetry: bool,
    quiet: bool,
    bar: Option<ProgressBar>,
    last_log_time: Mutex<Instant>,
    last_phase: Mutex<String>,
    total_discovered_cached: AtomicU64,
}

impl IndicatifTerminalProgress {
    pub fn new(json_telemetry: bool, quiet: bool) -> Self {
        let is_tty = std::io::stderr().is_terminal() && !json_telemetry && !quiet;

        let bar = if is_tty {
            let pb = ProgressBar::with_draw_target(None, ProgressDrawTarget::stderr());
            let style = ProgressStyle::default_bar()
                .template("{prefix:.bold} {spinner:.green} [{bar:25.cyan/blue}] {percent}% | {pos}/{len} items ({msg}) | {per_sec} | {bytes_per_sec} | Elapsed: {elapsed} | ETA: {eta}")
                .unwrap_or_else(|_| ProgressStyle::default_bar())
                .progress_chars("=> ");
            pb.set_style(style);
            pb.enable_steady_tick(Duration::from_millis(120));
            Some(pb)
        } else {
            None
        };

        Self {
            is_tty,
            json_telemetry,
            quiet,
            bar,
            last_log_time: Mutex::new(Instant::now()),
            last_phase: Mutex::new(String::new()),
            total_discovered_cached: AtomicU64::new(0),
        }
    }
}

impl ProgressSink for IndicatifTerminalProgress {
    fn render_progress(&self, snapshot: &ScanProgressSnapshot) {
        if self.quiet {
            return;
        }

        if self.json_telemetry {
            let json = serde_json::json!({
                "type": "progress",
                "session_id": snapshot.session_id,
                "phase": snapshot.active_phase.to_string(),
                "items_processed": snapshot.items_processed,
                "items_skipped": snapshot.items_skipped,
                "items_discovered": snapshot.items_discovered,
                "bytes_processed": snapshot.bytes_processed,
                "bytes_skipped": snapshot.bytes_skipped,
                "bytes_discovered": snapshot.bytes_discovered,
                "throughput_fps": snapshot.items_per_sec,
                "throughput_mbps": snapshot.mb_per_sec,
                "elapsed_secs": snapshot.elapsed_duration.as_secs_f64(),
                "eta_secs": snapshot.estimated_remaining.map(|d| d.as_secs_f64()),
                "errors": snapshot.errors_encountered,
                "warnings": snapshot.warnings_encountered,
            });
            eprintln!("{}", json);
            return;
        }

        if self.is_tty {
            if let Some(ref pb) = self.bar {
                let phase_str = format!("[{}]", snapshot.active_phase);
                pb.set_prefix(phase_str);

                let completed_items = snapshot.items_processed + snapshot.items_skipped;

                if snapshot.items_discovered > 0 {
                    let old_total = self
                        .total_discovered_cached
                        .swap(snapshot.items_discovered, Ordering::Relaxed);
                    if old_total != snapshot.items_discovered {
                        pb.set_length(snapshot.items_discovered);
                    }
                    pb.set_position(completed_items);
                }

                let percent = if snapshot.items_discovered > 0 {
                    (completed_items as f64 / snapshot.items_discovered as f64 * 100.0).min(100.0)
                } else {
                    0.0
                };

                let eta_str = match snapshot.estimated_remaining {
                    Some(rem) => format_duration(rem),
                    None => "--".to_string(),
                };

                let vol_str = if snapshot.bytes_discovered > 0 {
                    format!(
                        "{}/{}",
                        format_bytes(snapshot.bytes_processed + snapshot.bytes_skipped),
                        format_bytes(snapshot.bytes_discovered)
                    )
                } else {
                    format_bytes(snapshot.bytes_processed + snapshot.bytes_skipped)
                };

                let status_msg = if snapshot.items_skipped > 0 {
                    format!(
                        "{:.0}% | {}/{} items (Skipped: {} | Extracted: {}) | {:.1} items/s | {:.1} MB/s | Elapsed: {} | ETA: {}",
                        percent,
                        completed_items,
                        snapshot.items_discovered,
                        snapshot.items_skipped,
                        snapshot.items_processed,
                        snapshot.items_per_sec,
                        snapshot.mb_per_sec,
                        format_duration(snapshot.elapsed_duration),
                        eta_str,
                    )
                } else {
                    format!(
                        "{:.0}% | {}/{} items ({}) | {:.1} items/s | {:.1} MB/s | Elapsed: {} | ETA: {}",
                        percent,
                        snapshot.items_processed,
                        snapshot.items_discovered,
                        vol_str,
                        snapshot.items_per_sec,
                        snapshot.mb_per_sec,
                        format_duration(snapshot.elapsed_duration),
                        eta_str,
                    )
                };
                pb.set_message(status_msg);
            }
        } else {
            // Non-interactive fallback: emit line every 5 seconds or upon phase change
            let now = Instant::now();
            let mut last_time = self.last_log_time.lock().unwrap();
            let mut last_ph = self.last_phase.lock().unwrap();
            let current_phase_str = snapshot.active_phase.to_string();

            if now.duration_since(*last_time) >= Duration::from_secs(5)
                || *last_ph != current_phase_str
            {
                *last_time = now;
                *last_ph = current_phase_str.clone();

                let completed_items = snapshot.items_processed + snapshot.items_skipped;
                let percent = if snapshot.items_discovered > 0 {
                    (completed_items as f64 / snapshot.items_discovered as f64 * 100.0).min(100.0)
                } else {
                    0.0
                };

                let eta_str = match snapshot.estimated_remaining {
                    Some(rem) => format_duration(rem),
                    None => "--".to_string(),
                };

                if snapshot.items_skipped > 0 {
                    eprintln!(
                        "[Progress] Phase: {} | Processed: {}/{} (Skipped: {}) ({:.1}%) | Rate: {:.1} items/s ({:.1} MB/s) | Elapsed: {} | ETA: {} | Errors: {}",
                        current_phase_str,
                        completed_items,
                        snapshot.items_discovered,
                        snapshot.items_skipped,
                        percent,
                        snapshot.items_per_sec,
                        snapshot.mb_per_sec,
                        format_duration(snapshot.elapsed_duration),
                        eta_str,
                        snapshot.errors_encountered,
                    );
                } else {
                    eprintln!(
                        "[Progress] Phase: {} | Processed: {}/{} ({:.1}%) | Rate: {:.1} items/s ({:.1} MB/s) | Elapsed: {} | ETA: {} | Errors: {}",
                        current_phase_str,
                        snapshot.items_processed,
                        snapshot.items_discovered,
                        percent,
                        snapshot.items_per_sec,
                        snapshot.mb_per_sec,
                        format_duration(snapshot.elapsed_duration),
                        eta_str,
                        snapshot.errors_encountered,
                    );
                }
            }
        }
    }

    fn render_summary(&self, summary: &ExecutionSummary) {
        if let Some(ref pb) = self.bar {
            pb.finish_and_clear();
        }

        if self.json_telemetry {
            let json = serde_json::json!({
                "type": "summary",
                "session_id": summary.session_id,
                "target_path": summary.target_path,
                "mode": summary.mode.to_string(),
                "started_at": summary.started_at,
                "completed_at": summary.completed_at,
                "elapsed_secs": summary.elapsed_duration.as_secs_f64(),
                "total_discovered": summary.total_discovered,
                "total_processed": summary.total_processed,
                "total_skipped": summary.total_skipped,
                "total_bytes_processed": summary.total_bytes_processed,
                "total_bytes_skipped": summary.total_bytes_skipped,
                "force_reindex": summary.force_reindex,
                "average_fps": summary.average_items_per_sec,
                "average_mbps": summary.average_mb_per_sec,
                "db_upserted": summary.db_upserted,
                "db_modified": summary.db_modified,
                "db_matched": summary.db_matched,
                "db_failed": summary.db_failed,
                "errors": summary.extraction_errors,
                "warnings": summary.warnings_count,
                "was_cancelled": summary.was_cancelled,
            });
            eprintln!("{}", json);
            return;
        }

        if self.quiet {
            return;
        }

        let status_text = if summary.was_cancelled {
            "Cancelled early (interrupted)"
        } else if summary.extraction_errors > 0 {
            "Completed with errors"
        } else {
            "Completed successfully"
        };

        let rate_str = format!(
            "{:.1} items/sec ({:.1} MB/s)",
            summary.average_items_per_sec, summary.average_mb_per_sec
        );

        let mode_text = if summary.force_reindex {
            format!("{} (Forced re-index active)", summary.mode)
        } else if summary.total_skipped > 0 {
            format!("{} (Incremental change caching active)", summary.mode)
        } else {
            summary.mode.to_string()
        };

        let skipped_line = if summary.total_skipped > 0 {
            format!(
                "\nItems Skipped:       {} (Already indexed & unmodified)",
                summary.total_skipped
            )
        } else {
            String::new()
        };

        let volume_text = if summary.total_bytes_skipped > 0 {
            format!(
                "{} (Extracted: {} | Cached: {})",
                format_bytes(summary.total_bytes_processed + summary.total_bytes_skipped),
                format_bytes(summary.total_bytes_processed),
                format_bytes(summary.total_bytes_skipped)
            )
        } else {
            format!(
                "{} ({} bytes)",
                format_bytes(summary.total_bytes_processed),
                summary.total_bytes_processed
            )
        };

        let db_str = if summary.db_upserted > 0
            || summary.db_matched > 0
            || summary.db_failed > 0
            || summary.total_skipped > 0
        {
            let mut parts = Vec::new();
            if summary.db_upserted > 0 {
                parts.push(format!("{} upserted", summary.db_upserted));
            }
            if summary.db_modified > 0 {
                parts.push(format!("{} modified", summary.db_modified));
            }
            if summary.db_matched > 0 {
                parts.push(format!("{} matched", summary.db_matched));
            }
            if summary.total_skipped > 0 {
                parts.push(format!("{} unchanged", summary.total_skipped));
            }
            if summary.db_failed > 0 {
                parts.push(format!("{} failed", summary.db_failed));
            }
            if parts.is_empty() {
                "0 operations".to_string()
            } else {
                parts.join(" | ")
            }
        } else {
            "Not configured (stream / file output only)".to_string()
        };

        let summary_box = format!(
            r#"
================================================================================
Metadata Extraction Execution Summary
--------------------------------------------------------------------------------
Session ID:          {}
Target Path:         {}
Performance Mode:    {}
Total Duration:      {} ({:.2}s)
Items Discovered:    {}{{}}
Items Extracted:     {} ({:.1}%)
Data Volume:         {}
Average Throughput:  {}
Database Ingestion:  {}
Errors / Warnings:   {} errors, {} warnings
Status:              {}
================================================================================
"#,
            summary.session_id,
            summary.target_path.display(),
            mode_text,
            format_duration(summary.elapsed_duration),
            summary.elapsed_duration.as_secs_f64(),
            summary.total_discovered,
            summary.total_processed,
            if summary.total_discovered > 0 {
                (summary.total_processed as f64 / summary.total_discovered as f64 * 100.0)
                    .min(100.0)
            } else {
                100.0
            },
            volume_text,
            rate_str,
            db_str,
            summary.extraction_errors,
            summary.warnings_count,
            status_text
        );

        let final_output = summary_box.replace("{}", &skipped_line);
        eprintln!("{}", final_output.trim_start());
    }
}
