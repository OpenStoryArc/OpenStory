//! Split an `IngestBatch` so every publish stays under the bus payload cap.
//!
//! Root cause of the 2026-09-23 Grok publish failures (REQUIREMENTS E-05):
//! Grok transcripts carry hook-execution and tool-result lines up to 1 MB
//! each, so a hundred-event batch reaches 5 to 10 MB and NATS refuses it at
//! the 8 MB `max_payload` before it leaves the client. The watcher batches
//! by count; the bus must batch by bytes.

use crate::IngestBatch;

/// Serialized size of a batch, as NATS will see it.
pub fn batch_bytes(batch: &IngestBatch) -> usize {
    serde_json::to_vec(batch)
        .map(|v| v.len())
        .unwrap_or(usize::MAX)
}

/// Split `batch` into pieces whose serialized size is at most `max_bytes`,
/// preserving event order. A single event larger than the cap is returned
/// alone (it will fail loudly at publish, which is the honest outcome).
/// A batch already under the cap comes back as one piece.
pub fn split_batch(batch: IngestBatch, max_bytes: usize) -> Vec<IngestBatch> {
    if batch.events.len() <= 1 || batch_bytes(&batch) <= max_bytes {
        return vec![batch];
    }
    let mid = batch.events.len() / 2;
    let mut left = batch.events;
    let right = left.split_off(mid);
    let mk = |events| IngestBatch {
        session_id: batch.session_id.clone(),
        project_id: batch.project_id.clone(),
        events,
    };
    let mut out = split_batch(mk(left), max_bytes);
    out.extend(split_batch(mk(right), max_bytes));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_story_core::cloud_event::CloudEvent;
    use open_story_core::event_data::{AgentPayload, ClaudeCodePayload, EventData};

    fn event(i: usize, text_len: usize) -> CloudEvent {
        let mut payload = ClaudeCodePayload::new();
        payload.text = Some("x".repeat(text_len));
        let data = EventData::with_payload(
            serde_json::json!({}),
            0,
            "s".to_string(),
            AgentPayload::ClaudeCode(payload),
        );
        CloudEvent::new(
            "arc://test/s".into(),
            "io.arc.event".into(),
            data,
            Some("message.user.prompt".into()),
            Some(format!("e{i}")),
            Some("2026-09-23T00:00:00Z".to_string()),
            None,
            None,
            Some("claude-code".into()),
        )
    }

    fn batch(n: usize, text_len: usize) -> IngestBatch {
        IngestBatch {
            session_id: "s".into(),
            project_id: "p".into(),
            events: (0..n).map(|i| event(i, text_len)).collect(),
        }
    }

    mod when_a_batch_exceeds_the_payload_cap {
        use super::*;

        #[test]
        fn it_splits_into_ordered_pieces_each_under_the_cap() {
            let b = batch(100, 1_000); // ~100 KB of text plus envelope
            let cap = 20_000;
            let whole: Vec<String> = b.events.iter().map(|e| e.id.clone()).collect();
            let pieces = split_batch(b, cap);
            assert!(pieces.len() >= 6, "{} pieces", pieces.len());
            assert!(
                pieces.iter().all(|p| batch_bytes(p) <= cap),
                "every piece under the cap"
            );
            let seen: Vec<String> = pieces
                .iter()
                .flat_map(|p| p.events.iter().map(|e| e.id.clone()))
                .collect();
            assert_eq!(seen, whole, "order and completeness preserved");
            assert!(pieces
                .iter()
                .all(|p| p.session_id == "s" && p.project_id == "p"));
        }

        #[test]
        fn it_leaves_a_small_batch_whole() {
            let b = batch(10, 10);
            let pieces = split_batch(b, 1_000_000);
            assert_eq!(pieces.len(), 1);
            assert_eq!(pieces[0].events.len(), 10);
        }

        #[test]
        fn it_returns_a_single_oversized_event_alone() {
            let b = batch(3, 50_000);
            let pieces = split_batch(b, 10_000);
            assert_eq!(pieces.len(), 3, "each oversized event alone, never dropped");
            assert!(pieces.iter().all(|p| p.events.len() == 1));
        }
    }
}
