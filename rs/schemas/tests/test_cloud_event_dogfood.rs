//! Dogfood the CloudEvent schema against real events from the live
//! OpenStory instance. For every sampled event across every agent,
//! the committed schema must validate it.
//!
//! Run:
//!   cargo test -p open-story-schemas --test test_cloud_event_dogfood -- --ignored --nocapture
//!
//! Failures here are not "the test is broken" — they mean a real event
//! in the wild doesn't fit our declared schema. That's a gap to close
//! in the schema or the type definitions.

use std::collections::BTreeMap;

use open_story_schemas::load_schema;
use serde_json::Value;

const BASE: &str = "http://localhost:3002";
const SESSION_SAMPLE: usize = 40;

#[tokio::test]
#[ignore = "requires OpenStory running on localhost:3002"]
async fn every_live_event_validates_against_committed_schema() {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();

    let schema = load_schema("cloud_event.schema.json").expect("schema");
    let validator = jsonschema::validator_for(&schema).expect("compile");

    // Enumerate sessions
    let sessions_body: Value = client
        .get(format!("{BASE}/api/sessions"))
        .send()
        .await
        .expect("GET /api/sessions")
        .json()
        .await
        .expect("sessions json");
    let sessions = sessions_body
        .get("sessions")
        .and_then(|v| v.as_array())
        .expect("sessions array");
    let ids: Vec<String> = sessions
        .iter()
        .take(SESSION_SAMPLE)
        .filter_map(|s| s.get("session_id").and_then(|v| v.as_str()).map(String::from))
        .collect();
    assert!(!ids.is_empty(), "no sessions");

    // Aggregate validation stats
    let mut total = 0usize;
    let mut by_agent: BTreeMap<String, (usize, usize)> = BTreeMap::new(); // agent → (valid, invalid)
    let mut first_failures: Vec<(String, Value, String)> = Vec::new();

    for sid in &ids {
        let url = format!("{BASE}/api/sessions/{sid}/events");
        let events: Vec<Value> = match client.get(&url).send().await {
            Ok(r) => r.json().await.unwrap_or_default(),
            Err(_) => continue,
        };
        for ev in events {
            total += 1;
            let agent = ev
                .get("agent")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            let entry = by_agent.entry(agent.clone()).or_insert((0, 0));
            let errors: Vec<_> = validator.iter_errors(&ev).collect();
            if errors.is_empty() {
                entry.0 += 1;
            } else {
                entry.1 += 1;
                if first_failures.len() < 5 {
                    let msg = errors
                        .iter()
                        .map(|e| format!("  {}: {}", e.instance_path, e))
                        .collect::<Vec<_>>()
                        .join("\n");
                    first_failures.push((
                        agent,
                        ev.get("id").cloned().unwrap_or(Value::Null),
                        msg,
                    ));
                }
            }
        }
    }

    eprintln!("\n── validation by agent ──");
    for (agent, (ok, bad)) in &by_agent {
        let total_agent = ok + bad;
        eprintln!(
            "  {agent:<12}  ok: {ok:>6}   invalid: {bad:>4}   total: {total_agent}"
        );
    }
    eprintln!("  ────────────────────────────────────");
    eprintln!("  total events: {total}");

    if !first_failures.is_empty() {
        eprintln!("\n❌ first {} validation failures:", first_failures.len());
        for (agent, id, msg) in &first_failures {
            eprintln!("  [{agent}] id={id}");
            eprintln!("{msg}");
        }
    }

    let total_invalid: usize = by_agent.values().map(|(_, b)| b).sum();
    assert_eq!(
        total_invalid, 0,
        "{total_invalid} live event(s) did not validate — schema or types need a fix"
    );
}
