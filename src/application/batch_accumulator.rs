use tokio::sync::mpsc;
use uuid::Uuid;

use crate::domain::models::{DiscoveredMediaItem, DiscoveryBatch};

/// Accumulates media items into fixed-size batches and streams them over a bounded channel.
pub struct BatchAccumulator {
    session_id: Uuid,
    batch_size: usize,
    current_sequence: u64,
    buffer: Vec<DiscoveredMediaItem>,
    tx: mpsc::Sender<DiscoveryBatch>,
}

impl BatchAccumulator {
    pub fn new(session_id: Uuid, batch_size: usize, tx: mpsc::Sender<DiscoveryBatch>) -> Self {
        Self {
            session_id,
            batch_size: batch_size.max(1),
            current_sequence: 1,
            buffer: Vec::with_capacity(batch_size),
            tx,
        }
    }

    /// Pushes an item into the buffer. If buffer capacity reaches `batch_size`,
    /// emits the batch over the channel (awaiting on backpressure if channel is full).
    pub async fn push(
        &mut self,
        item: DiscoveredMediaItem,
    ) -> Result<(), mpsc::error::SendError<DiscoveryBatch>> {
        self.buffer.push(item);
        if self.buffer.len() >= self.batch_size {
            self.emit_batch().await?;
        }
        Ok(())
    }

    /// Flushes any remaining items in the buffer as a final batch.
    pub async fn flush(&mut self) -> Result<(), mpsc::error::SendError<DiscoveryBatch>> {
        if !self.buffer.is_empty() {
            self.emit_batch().await?;
        }
        Ok(())
    }

    async fn emit_batch(&mut self) -> Result<(), mpsc::error::SendError<DiscoveryBatch>> {
        let items = std::mem::replace(&mut self.buffer, Vec::with_capacity(self.batch_size));
        let batch = DiscoveryBatch::new(self.current_sequence, self.session_id, items);
        self.current_sequence += 1;
        self.tx.send(batch).await
    }
}
