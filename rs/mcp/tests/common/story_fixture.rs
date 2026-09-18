//! Seed a temp store with a story golden: its events, plus every pattern
//! the default pipeline (eval-apply, sentence, story) folds from them.
//! Memory hands, group B.

use open_story_core::cloud_event::CloudEvent;
use open_story_patterns::PatternPipeline;
use open_story_store::event_store::{EventStore, SessionRow};
use std::path::PathBuf;
use std::sync::Arc;

pub fn golden_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../patterns/tests/fixtures/story")
        .join(name)
}

pub fn golden_events(name: &str) -> Vec<serde_json::Value> {
    let path = golden_dir(name).join("events.jsonl");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

pub fn golden_expected(name: &str) -> serde_json::Value {
    let path = golden_dir(name).join("expected.json");
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap()
}

/// Insert a golden's events and its folded patterns; returns the session id.
pub async fn seed_golden(store: &Arc<dyn EventStore>, name: &str) -> String {
    let events = golden_events(name);
    let sid = events[0]["data"]["session_id"]
        .as_str()
        .unwrap()
        .to_string();
    store.insert_batch(&sid, &events).await.unwrap();
    let row: SessionRow = serde_json::from_value(serde_json::json!({
        "id": sid,
        "event_count": events.len(),
        "first_event": events[0]["time"],
        "last_event": events[events.len() - 1]["time"],
    }))
    .unwrap();
    store.upsert_session(&row).await.unwrap();

    let mut pipeline = PatternPipeline::new();
    let mut patterns = Vec::new();
    for value in &events {
        let ev: CloudEvent = serde_json::from_value(value.clone()).unwrap();
        patterns.extend(pipeline.feed_event(&ev).0);
    }
    patterns.extend(pipeline.flush().0);
    for p in &patterns {
        store.insert_pattern(&sid, p).await.unwrap();
    }
    sid
}
