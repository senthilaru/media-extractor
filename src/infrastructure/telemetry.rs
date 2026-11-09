use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::domain::models::{AtomicSessionCounters, SessionMetrics};

/// Background task that emits periodic heartbeat metrics.
pub struct HeartbeatReporter {
    session_id: Uuid,
    counters: Arc<AtomicSessionCounters>,
    started_at: SystemTime,
    interval: Duration,
    tx: mpsc::Sender<SessionMetrics>,
}

impl HeartbeatReporter {
    pub fn new(
        session_id: Uuid,
        counters: Arc<AtomicSessionCounters>,
        started_at: SystemTime,
        interval: Duration,
        tx: mpsc::Sender<SessionMetrics>,
    ) -> Self {
        Self {
            session_id,
            counters,
            started_at,
            interval,
            tx,
        }
    }

    /// Spawns the telemetry heartbeat loop into a tokio background task.
    pub fn spawn(
        self,
        queue_depth_fn: impl Fn() -> usize + Send + 'static,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(self.interval);
            // Skip immediate tick
            ticker.tick().await;

            loop {
                ticker.tick().await;

                let depth = queue_depth_fn();
                let snapshot = SessionMetrics::from_counters(
                    self.session_id,
                    &self.counters,
                    self.started_at,
                    depth,
                );

                if self.tx.send(snapshot).await.is_err() {
                    // Receiver closed
                    break;
                }
            }
        })
    }
}
