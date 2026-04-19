//! Dogfood ViewRecord + WireRecord schemas against the live OpenStory
//! REST API.
//!
//! `/view-records` returns ViewRecords; `/records` returns WireRecords.
//!
//! Run:
//!   cargo test -p open-story-schemas --test test_view_record_dogfood -- --ignored --nocapture

use open_story_schemas::load_schema;
use serde_json::Value;
use std::collections::BTreeMap;

const BASE: &str = "http://localhost:3002";
const SESSION_SAMPLE: usize = 30;

async fn session_ids(client: &reqwest::Client) -> Vec<String> {
    let body: Value = client
        .get(format!("{BASE}/api/sessions"))
        .send()
        .await
        .expect("GET /api/sessions")
        .json()
        .await
        .expect("sessions json");
    body.get("sessions")
        .and_then(|v| v.as_array())
        .unwrap_or(&Vec::new())
        .iter()
        .take(SESSION_SAMPLE)
        .filter_map(|s| s.get("session_id").and_then(|v| v.as_str()).map(String::from))
        .collect()
}

async fn fetch_array(client: &reqwest::Client, url: &str) -> Vec<Value> {
    match client.get(url).send().await {
        Ok(r) => r.json().await.unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

fn dogfood_schema(
    client_builder: reqwest::Client,
    schema_name: &'static str,
    label: &'static str,
    endpoint: &'static str,
) -> impl std::future::Future<Output = ()> {
    async move {
        let schema = load_schema(schema_name).expect("schema");
        let validator = jsonschema::validator_for(&schema).expect("compile");
        let ids = session_ids(&client_builder).await;
        assert!(!ids.is_empty(), "no sessions");

        let mut total = 0usize;
        let mut by_record_type: BTreeMap<String, (usize, usize)> = BTreeMap::new();
        let mut first_failures: Vec<(String, Value, String)> = Vec::new();

        for sid in &ids {
            let url = format!("{BASE}/api/sessions/{sid}/{endpoint}");
            let records = fetch_array(&client_builder, &url).await;
            for r in records {
                total += 1;
                let rt = r
                    .get("record_type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("(missing)")
                    .to_string();
                let entry = by_record_type.entry(rt.clone()).or_insert((0, 0));
                let errors: Vec<_> = validator.iter_errors(&r).collect();
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
                        first_failures.push((rt, r.clone(), msg));
                    }
                }
            }
        }

        eprintln!("\n── {label} validation by record_type ──");
        for (rt, (ok, bad)) in &by_record_type {
            eprintln!("  {rt:<24}  ok: {ok:>6}   invalid: {bad:>4}");
        }
        eprintln!("  total: {total}");

        if !first_failures.is_empty() {
            eprintln!("\n❌ sample failures:");
            for (rt, r, msg) in &first_failures {
                eprintln!(
                    "  [{rt}] id={}",
                    r.get("id").and_then(|v| v.as_str()).unwrap_or("?")
                );
                eprintln!("{msg}");
            }
        }

        let invalid: usize = by_record_type.values().map(|(_, b)| b).sum();
        assert_eq!(invalid, 0, "{invalid} live {label}(s) did not validate");
    }
}

#[tokio::test]
#[ignore = "requires OpenStory on localhost:3002"]
async fn every_live_view_record_validates() {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    dogfood_schema(client, "view_record.schema.json", "view_record", "view-records").await;
}

#[tokio::test]
#[ignore = "requires OpenStory on localhost:3002"]
async fn every_live_wire_record_validates() {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .unwrap();
    dogfood_schema(client, "wire_record.schema.json", "wire_record", "records").await;
}
