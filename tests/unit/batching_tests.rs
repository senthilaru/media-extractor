use std::path::PathBuf;
use tokio::sync::mpsc;
use uuid::Uuid;

use metadata_extractor::application::batch_accumulator::BatchAccumulator;
use metadata_extractor::domain::models::{DiscoveredMediaItem, MediaCategory};

#[tokio::test]
async fn test_batch_accumulator_chunking() {
    let session_id = Uuid::new_v4();
    let (tx, mut rx) = mpsc::channel(10);
    let mut accumulator = BatchAccumulator::new(session_id, 3, tx);

    for i in 1..=7 {
        let item = DiscoveredMediaItem::new(
            PathBuf::from(format!("/media/{}.jpg", i)),
            1000 * i,
            None,
            MediaCategory::Photo,
            "jpg",
        );
        accumulator.push(item).await.unwrap();
    }

    // Flush remaining 1 item (7 items with batch size 3 -> batches of 3, 3, 1)
    accumulator.flush().await.unwrap();

    let batch1 = rx.recv().await.expect("batch 1 expected");
    assert_eq!(batch1.batch_sequence, 1);
    assert_eq!(batch1.count, 3);
    assert_eq!(batch1.items.len(), 3);

    let batch2 = rx.recv().await.expect("batch 2 expected");
    assert_eq!(batch2.batch_sequence, 2);
    assert_eq!(batch2.count, 3);
    assert_eq!(batch2.items.len(), 3);

    let batch3 = rx.recv().await.expect("batch 3 expected");
    assert_eq!(batch3.batch_sequence, 3);
    assert_eq!(batch3.count, 1);
    assert_eq!(batch3.items.len(), 1);

    // Drop accumulator and verify stream ends
    drop(accumulator);
    assert!(rx.recv().await.is_none());
}
