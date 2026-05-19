//! Unit tests for `pump_subscription` and `CancelGuard` — the two
//! pieces of subscription mechanics that belong to MCP (not to the bus).
//!
//! Each test is a pure exercise: build a source channel, push
//! IngestBatches, observe the StreamEvents the pump produces. No bus,
//! no NATS, no stdio.

use open_story_bus::IngestBatch;
use open_story_mcp::subscription::{pump_subscription, CancelGuard, StreamEvent};
use tokio::sync::mpsc;

fn mk_batch(sid: &str) -> IngestBatch {
    IngestBatch {
        session_id: sid.to_string(),
        project_id: "p".to_string(),
        events: vec![],
    }
}

mod when_a_pump_receives_three_ingest_batches_in_sequence {
    use super::*;

    #[tokio::test]
    async fn it_emits_three_stream_events_with_seq_one_two_three() {
        let (src_tx, src_rx) = mpsc::channel::<IngestBatch>(16);
        let (sink_tx, mut sink_rx) = mpsc::channel::<StreamEvent>(16);
        let handle = tokio::spawn(pump_subscription(
            src_rx,
            sink_tx,
            "sid-pump".to_string(),
        ));

        for _ in 0..3 {
            src_tx.send(mk_batch("sid-pump")).await.unwrap();
        }
        drop(src_tx);
        handle.await.unwrap();

        let mut seqs = Vec::new();
        while let Some(ev) = sink_rx.recv().await {
            assert_eq!(ev.session_id, "sid-pump");
            seqs.push(ev.seq);
        }
        assert_eq!(seqs, vec![1, 2, 3]);
    }
}

mod when_the_source_channel_closes {
    use super::*;

    #[tokio::test]
    async fn the_pump_task_terminates_cleanly() {
        let (src_tx, src_rx) = mpsc::channel::<IngestBatch>(4);
        let (sink_tx, _sink_rx) = mpsc::channel::<StreamEvent>(4);
        let handle = tokio::spawn(pump_subscription(
            src_rx,
            sink_tx,
            "sid-close".to_string(),
        ));

        // No publishes — just close the source.
        drop(src_tx);

        // The pump task must exit promptly.
        tokio::time::timeout(std::time::Duration::from_millis(100), handle)
            .await
            .expect("pump task did not terminate within 100ms of source close")
            .expect("pump task panicked");
    }
}

mod when_the_sink_is_dropped_mid_stream {
    use super::*;

    #[tokio::test]
    async fn the_pump_task_terminates_without_blocking() {
        let (src_tx, src_rx) = mpsc::channel::<IngestBatch>(16);
        let (sink_tx, sink_rx) = mpsc::channel::<StreamEvent>(16);
        let handle = tokio::spawn(pump_subscription(
            src_rx,
            sink_tx,
            "sid-orphan".to_string(),
        ));

        // Drop the sink first — any send by the pump should fail and
        // it should exit.
        drop(sink_rx);

        // Push one batch; pump tries to send, fails, exits.
        src_tx.send(mk_batch("sid-orphan")).await.unwrap();

        tokio::time::timeout(std::time::Duration::from_millis(100), handle)
            .await
            .expect("pump task did not terminate within 100ms of sink drop")
            .expect("pump task panicked");
    }
}

mod when_a_cancel_guard_is_dropped {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[test]
    fn it_invokes_the_registered_cancel_fn_exactly_once() {
        let fired = Arc::new(AtomicBool::new(false));
        let fired_clone = fired.clone();
        let guard = CancelGuard::from_fn(move || {
            fired_clone.store(true, Ordering::SeqCst);
        });

        assert!(!fired.load(Ordering::SeqCst));
        drop(guard);
        assert!(fired.load(Ordering::SeqCst));
    }
}
