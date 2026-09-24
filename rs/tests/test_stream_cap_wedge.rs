//! K-08: a node with a tiny events cap is flooded; `/api/health` flips to
//! critical (`stream_cap:events`) while ingestion keeps answering. Needs
//! Docker and the image built from this tree:
//!   docker build -t open-story:test ./rs
//! The test name carries `container` so CI's skip filter leaves it out.
//! Scoreboard: docs/research/openstory-as-node/REQUIREMENTS.md

mod helpers;

use helpers::container::start_open_story_with_env;
use helpers::synth::generate_fixture_dir;
use serde_json::Value;
use std::time::{Duration, Instant};

#[tokio::test]
async fn container_stream_cap_flips_critical_before_ingestion_wedges() {
    // Forty small sessions (about 30 KB each on the wire) against a 1 MiB
    // cap: dozens of batches fit, so the stream fills past 90 % and stays
    // there under discard-old. One big batch per session would never do
    // that; each would evict the last and the fill would sit at one batch.
    let tmp = tempfile::tempdir().unwrap();
    generate_fixture_dir(tmp.path(), 40, 20, 1_000);
    let server =
        start_open_story_with_env(tmp.path(), &[("OPEN_STORY_EVENTS_MAX_BYTES", "1048576")]).await;
    let base = server.base_url();
    let client = reqwest::Client::new();

    let deadline = Instant::now() + Duration::from_secs(180);
    let mut last = Value::Null;
    loop {
        if let Ok(resp) = client.get(format!("{base}/api/health")).send().await {
            if let Ok(body) = resp.json::<Value>().await {
                last = body;
                let critical = last["verdict"]["level"] == "critical";
                let cap = last["verdict"]["findings"]
                    .as_array()
                    .map(|f| f.iter().any(|x| x["id"] == "stream_cap:events"))
                    .unwrap_or(false);
                if critical && cap {
                    break;
                }
            }
        }
        assert!(
            Instant::now() < deadline,
            "health never flipped to critical on the events cap; last: {last}"
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    let events = last["streams"]
        .as_array()
        .and_then(|s| s.iter().find(|x| x["name"] == "events").cloned())
        .expect("events stream reported");
    assert_eq!(
        events["max_bytes"], 1_048_576,
        "the cap the env set: {events}"
    );
    assert!(events["percent"].as_f64().unwrap() >= 0.9, "{events}");
    eprintln!("K-08 events stream at the flip: {events}");

    // Ingestion did not wedge: the API answers and holds sessions.
    let sessions: Value = client
        .get(format!("{base}/api/sessions"))
        .send()
        .await
        .expect("sessions answers")
        .json()
        .await
        .unwrap();
    let n = sessions["sessions"]
        .as_array()
        .map(|a| a.len())
        .or_else(|| sessions.as_array().map(|a| a.len()))
        .unwrap_or(0);
    assert!(
        n >= 1,
        "sessions still served under a full stream: {sessions}"
    );
    assert_eq!(
        last["bus"]["connected"], true,
        "the bus stayed up: {}",
        last["bus"]
    );
}
