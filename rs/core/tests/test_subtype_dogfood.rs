//! Dogfood test — every subtype currently persisted in the local
//! OpenStory instance must parse into a `Subtype` variant.
//!
//! Run manually (requires OpenStory running on localhost:3002):
//!   cargo test -p open-story-core --test test_subtype_dogfood -- --ignored --nocapture
//!
//! This is the real validator for the enum: any subtype flowing through
//! production today that the enum doesn't know about is a gap we need
//! to close. Treat failure as "go add the variant to the enum," not
//! "the test is broken."

use std::collections::BTreeMap;

use open_story_core::subtype::Subtype;
use serde_json::Value;
use std::str::FromStr;

const OPEN_STORY_BASE: &str = "http://localhost:3002";
/// How many sessions to sample (most-recent first). OpenStory often holds
/// hundreds; we don't need to walk them all to get full coverage.
const SESSION_SAMPLE: usize = 60;

#[tokio::test]
#[ignore = "requires a running OpenStory on localhost:3002"]
async fn every_live_subtype_parses_into_the_enum() {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .expect("build reqwest client");

    // 1. Enumerate sessions
    let sessions_body: Value = client
        .get(format!("{OPEN_STORY_BASE}/api/sessions"))
        .send()
        .await
        .expect("GET /api/sessions — is OpenStory running?")
        .json()
        .await
        .expect("sessions JSON");

    let sessions = sessions_body
        .get("sessions")
        .and_then(|v| v.as_array())
        .expect("sessions array");

    let session_ids: Vec<String> = sessions
        .iter()
        .take(SESSION_SAMPLE)
        .filter_map(|s| s.get("session_id").and_then(|v| v.as_str()).map(String::from))
        .collect();

    assert!(!session_ids.is_empty(), "no sessions found");

    // 2. Aggregate distinct subtypes with counts, grouped by agent
    //    Keyed as (agent, subtype) → count.
    let mut by_agent: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();

    for sid in &session_ids {
        let url = format!("{OPEN_STORY_BASE}/api/sessions/{sid}/events");
        let events: Vec<Value> = match client.get(&url).send().await {
            Ok(r) => r.json().await.unwrap_or_default(),
            Err(_) => continue,
        };
        for e in &events {
            let agent = e
                .get("agent")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();
            if let Some(st) = e.get("subtype").and_then(|v| v.as_str()) {
                *counts.entry(st.to_string()).or_insert(0) += 1;
                *by_agent.entry((agent, st.to_string())).or_insert(0) += 1;
            }
        }
    }

    // Show per-agent breakdown so pi-mono / hermes coverage is visible.
    let mut agents: std::collections::BTreeSet<&String> =
        by_agent.keys().map(|(a, _)| a).collect();
    for agent in &agents {
        eprintln!("\n── agent: {agent} ──");
        for ((a, st), n) in &by_agent {
            if a == *agent {
                eprintln!("  {n:>8}  {st}");
            }
        }
    }
    if agents.iter().any(|a| a.as_str() == "pi-mono") {
        eprintln!("\n✓ pi-mono data present in sample");
    } else {
        eprintln!("\nNOTE: no pi-mono events in this sample — run a pi-mono session first if you want cross-agent coverage");
    }
    let _ = &mut agents; // silence unused_mut if compiler strict

    // 3. Assert every one parses
    let unknown: Vec<(String, usize)> = counts
        .iter()
        .filter_map(|(st, n)| {
            if Subtype::from_str(st).is_err() {
                Some((st.clone(), *n))
            } else {
                None
            }
        })
        .collect();

    if !unknown.is_empty() {
        eprintln!("\n❌ subtypes the enum does NOT know about:");
        for (st, n) in &unknown {
            eprintln!("  {n:>8}  {st}");
        }
        panic!(
            "{} live subtype(s) don't parse into Subtype — enum is incomplete",
            unknown.len()
        );
    }

    eprintln!("\n✓ every live subtype parses into Subtype");
}

// ── Fixture-based pi-mono dogfood (no server required) ─────────────────
//
// Runs against the real captured pi-mono sessions under rs/tests/fixtures/
// pi_mono/. For each JSONL line, translate it to CloudEvents and assert
// every produced subtype parses into Subtype.
//
// This is the deterministic cousin of the HTTP dogfood test — CI-safe,
// no network.

use open_story_core::reader::read_new_lines;
use open_story_core::translate::TranscriptState;
use std::path::PathBuf;

fn pi_mono_fixture_paths() -> Vec<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("tests/fixtures/pi_mono");
    std::fs::read_dir(&dir)
        .unwrap_or_else(|_| panic!("pi-mono fixtures at {}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "jsonl").unwrap_or(false))
        .collect()
}

#[test]
fn every_subtype_in_pi_mono_fixtures_parses_into_the_enum() {
    let fixtures = pi_mono_fixture_paths();
    assert!(!fixtures.is_empty(), "no pi-mono fixtures found");

    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut unknown: Vec<(String, PathBuf)> = Vec::new();

    for path in &fixtures {
        let mut state = TranscriptState::new(format!(
            "dogfood-{}",
            path.file_stem().unwrap().to_string_lossy()
        ));
        let events = read_new_lines(path, &mut state)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        for ce in &events {
            if let Some(st) = ce.subtype.as_deref() {
                *counts.entry(st.to_string()).or_insert(0) += 1;
                if Subtype::from_str(st).is_err() {
                    unknown.push((st.to_string(), path.clone()));
                }
            }
        }
    }

    eprintln!("\n── pi-mono fixture subtype distribution ──");
    for (st, n) in &counts {
        eprintln!("  {n:>6}  {st}");
    }

    if !unknown.is_empty() {
        eprintln!("\n❌ pi-mono subtypes the enum doesn't know:");
        for (st, path) in &unknown {
            eprintln!("  {st}  (in {})", path.file_name().unwrap().to_string_lossy());
        }
        panic!("{} unknown pi-mono subtype(s)", unknown.len());
    }
}
