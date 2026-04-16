//! PRINCIPLE TEST — "OpenStory observes its own development legibly."
//!
//! The recursive principle: OpenStory exists to observe coding agents.
//! We are building it by observing me as a coding agent. The system's
//! ability to render its own development sessions as legible
//! subject-verb-object sentences IS its core value proposition. If
//! OpenStory can't tell the story of how OpenStory got built, the
//! principle has failed silently.
//!
//! This test queries the live OpenStory instance, fetches its own
//! recent session data, and asserts that the turn-sentence detector
//! still produces legible output. If the eval-apply detector or
//! sentence builder regresses enough to stop telling the story —
//! even while every unit test still passes — this test catches it.
//!
//! See docs/research/architecture-audit/PRINCIPLES.md for the broader
//! principle-test pattern. This is the recursive one.
//!
//! Run:
//!   cargo test --test test_principle_recursive_observability \
//!     -- --ignored --nocapture

use serde_json::Value;
use std::collections::HashMap;

const BASE: &str = "http://localhost:3002";

/// Sessions smaller than this are skipped — tiny test sessions distort
/// the legibility ratio without representing real work.
const MIN_EVENT_COUNT: u64 = 50;

/// Of the sentences a session emits, what fraction must pass all four
/// legibility checks. 80% leaves room for legitimately thin turns
/// (one-word prompts, pure handoffs) without rewarding pervasive
/// degeneracy.
const LEGIBILITY_THRESHOLD: f64 = 0.80;

/// Sentences shorter than this are flagged as degenerate.
const MIN_SUMMARY_CHARS: usize = 30;

#[derive(Debug)]
struct SessionReport {
    id: String,
    label: Option<String>,
    sentence_count: usize,
    legible_count: usize,
    legibility_ratio: f64,
    /// Three worst sentence summaries from this session.
    worst: Vec<String>,
}

#[tokio::test]
#[ignore = "requires OpenStory running on localhost:3002"]
async fn openstory_observes_its_own_development_sessions_legibly() {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("build reqwest client");

    let sessions = fetch_sessions(&client).await;

    let qualifying: Vec<&Value> = sessions
        .iter()
        .filter(|s| {
            s.get("event_count")
                .and_then(|v| v.as_u64())
                .map(|n| n >= MIN_EVENT_COUNT)
                .unwrap_or(false)
        })
        .collect();

    eprintln!(
        "\n── recursive observability principle test ──\n  total sessions: {}\n  qualifying (>= {} events): {}",
        sessions.len(),
        MIN_EVENT_COUNT,
        qualifying.len()
    );

    assert!(
        !qualifying.is_empty(),
        "no qualifying sessions found (need >= {MIN_EVENT_COUNT} events). \
         If the instance is empty this test cannot validate the principle — \
         run a real session first."
    );

    let mut reports: Vec<SessionReport> = Vec::new();
    let mut total_sentences = 0usize;
    let mut total_legible = 0usize;
    /// Sessions with zero turn.complete events — the eval-apply state
    /// machine never gets a turn boundary so no sentence can crystallize.
    /// Pi-mono is known to fall here (doesn't emit turn.complete).
    /// Claude Code sessions ingested watcher-only (no Stop hook) also
    /// fall here.
    let mut silent_no_turn_boundary: Vec<(String, String, String)> = Vec::new();
    /// Sessions with turn.complete events but ZERO turn.sentence
    /// patterns — the boundary fired but the detector didn't render.
    /// This is the "patterns consumer wasn't subscribed when this
    /// session was ingested" case (or a bug downstream of detection).
    let mut silent_despite_turn_boundary: Vec<(String, String, String, usize)> = Vec::new();

    for session in &qualifying {
        let id = session.get("session_id").and_then(|v| v.as_str()).unwrap_or("?").to_string();
        let label = session.get("label").and_then(|v| v.as_str()).map(String::from);

        let patterns = fetch_patterns(&client, &id).await;
        let sentences: Vec<&Value> = patterns
            .iter()
            .filter(|p| p.get("pattern_type").and_then(|v| v.as_str()) == Some("turn.sentence"))
            .collect();

        if sentences.is_empty() {
            // Distinguish the two failure modes by asking the events endpoint
            // about turn.complete count + agent.
            let (turn_completes, agent) = fetch_turn_complete_count_and_agent(&client, &id).await;
            let label_str = label.clone().unwrap_or_default();
            if turn_completes == 0 {
                silent_no_turn_boundary.push((id.clone(), agent, label_str));
            } else {
                silent_despite_turn_boundary.push((id.clone(), agent, label_str, turn_completes));
            }
            continue;
        }

        let mut legible = 0usize;
        let mut illegible_examples: Vec<(usize, String)> = Vec::new();

        for s in &sentences {
            let summary = s.get("summary").and_then(|v| v.as_str()).unwrap_or("");
            let metadata = s.get("metadata").cloned().unwrap_or(Value::Null);
            let issues = legibility_issues(summary, &metadata);
            if issues.is_empty() {
                legible += 1;
            } else if illegible_examples.len() < 3 {
                illegible_examples.push((issues.len(), summary.to_string()));
            }
        }

        let ratio = legible as f64 / sentences.len() as f64;
        let worst = illegible_examples
            .into_iter()
            .map(|(_, s)| s)
            .collect::<Vec<_>>();

        total_sentences += sentences.len();
        total_legible += legible;

        reports.push(SessionReport {
            id: id.clone(),
            label,
            sentence_count: sentences.len(),
            legible_count: legible,
            legibility_ratio: ratio,
            worst,
        });
    }

    // Sort reports by legibility ratio ascending — worst first.
    reports.sort_by(|a, b| a.legibility_ratio.partial_cmp(&b.legibility_ratio).unwrap());

    eprintln!("\n  per-session legibility (worst first):");
    for r in &reports {
        let label_full = r.label.as_deref().unwrap_or("(no label)");
        let label_short = open_story_core::strings::truncate_at_char_boundary(label_full, 50);
        eprintln!(
            "    {:.0}%  {:>4}/{:<4}  {}  {label_short}",
            r.legibility_ratio * 100.0,
            r.legible_count,
            r.sentence_count,
            short(&r.id),
        );
    }

    let aggregate_ratio = if total_sentences == 0 {
        0.0
    } else {
        total_legible as f64 / total_sentences as f64
    };
    eprintln!(
        "\n  aggregate: {:>4}/{:<4} = {:.1}% legible across {} sessions",
        total_legible,
        total_sentences,
        aggregate_ratio * 100.0,
        reports.len()
    );

    if !silent_no_turn_boundary.is_empty() {
        eprintln!(
            "\n  ⚠ {} session(s) had ZERO turn.complete events — eval-apply never \
             saw a turn boundary, so no sentence could ever crystallize.",
            silent_no_turn_boundary.len()
        );
        eprintln!(
            "    (pi-mono doesn't emit turn.complete; claude-code sessions ingested \
             via the watcher path without Stop hooks fall here too)"
        );
        for (id, agent, label) in silent_no_turn_boundary.iter().take(8) {
            let label_short = open_story_core::strings::truncate_at_char_boundary(label, 50);
            eprintln!("    [{agent:>11}]  {}  {label_short}", short(id));
        }
    }
    if !silent_despite_turn_boundary.is_empty() {
        eprintln!(
            "\n  ❌ {} session(s) had turn.complete events but produced ZERO \
             turn.sentence patterns. Detector ran on per-event but never rendered \
             a sentence. Likely cause: the patterns consumer wasn't subscribed \
             when this session was ingested, or a bug downstream of eval-apply.",
            silent_despite_turn_boundary.len()
        );
        for (id, agent, label, n) in silent_despite_turn_boundary.iter().take(8) {
            let label_short = open_story_core::strings::truncate_at_char_boundary(label, 50);
            eprintln!(
                "    [{agent:>11}]  {}  {n} turn.complete events, 0 sentences  {label_short}",
                short(id)
            );
        }
    }

    let worst_session = reports.first();
    if let Some(w) = worst_session {
        if w.legibility_ratio < LEGIBILITY_THRESHOLD && !w.worst.is_empty() {
            eprintln!("\n  worst-session degenerate sentence examples ({}):", short(&w.id));
            for s in &w.worst {
                eprintln!("    {s}");
            }
        }
    }

    // ── Assertions ────────────────────────────────────────────────────

    assert!(
        total_sentences > 0,
        "qualifying sessions exist but ZERO turn.sentence patterns were produced. \
         The detector is silent — the principle is broken."
    );

    // Sessions silent for principled reasons (no turn.complete = no
    // boundary = no sentence by construction) are documented but not
    // flagged here. The principle violation is: sessions where the
    // boundary fired but the detector silently skipped. Those mean the
    // detector / persistence path is broken for live observability.
    let silent_despite_boundary_ratio =
        silent_despite_turn_boundary.len() as f64 / qualifying.len() as f64;
    assert!(
        silent_despite_boundary_ratio < 0.10,
        "{}/{} qualifying sessions had turn.complete events but produced ZERO \
         sentences ({:.0}%). The pattern-detection or persistence path silently \
         dropped sentences that should have been rendered. See the report above \
         for the specific session ids.",
        silent_despite_turn_boundary.len(),
        qualifying.len(),
        silent_despite_boundary_ratio * 100.0
    );

    // Pi-mono sessions — there should be a separate, named issue: the
    // story-rendering pipeline doesn't work for pi-mono today. Surface
    // it as a soft signal rather than a hard fail because it's a known
    // architectural gap (turn.complete is Claude Code-specific).
    let pi_mono_silent = silent_no_turn_boundary
        .iter()
        .filter(|(_, agent, _)| agent == "pi-mono")
        .count();
    if pi_mono_silent > 0 {
        eprintln!(
            "\n  ℹ {} pi-mono session(s) cannot produce sentences today because \
             pi-mono doesn't emit system.turn.complete. Pi-mono storytelling \
             requires an alternate turn-boundary signal — see BACKLOG.",
            pi_mono_silent
        );
    }

    assert!(
        aggregate_ratio >= LEGIBILITY_THRESHOLD,
        "aggregate sentence legibility is {:.1}%, below the {:.0}% threshold. \
         OpenStory is producing sentences but they don't tell the story — \
         the recursive principle has degraded.",
        aggregate_ratio * 100.0,
        LEGIBILITY_THRESHOLD * 100.0
    );

    eprintln!("\n  ✓ recursive observability holds — OpenStory still tells its own story");
}

// ── Helpers ───────────────────────────────────────────────────────────

async fn fetch_sessions(client: &reqwest::Client) -> Vec<Value> {
    let body: Value = client
        .get(format!("{BASE}/api/sessions"))
        .send()
        .await
        .expect("GET /api/sessions failed — is OpenStory running?")
        .json()
        .await
        .expect("sessions response is not JSON");
    body.get("sessions")
        .and_then(|v| v.as_array())
        .or_else(|| body.as_array())
        .cloned()
        .unwrap_or_default()
}

/// Returns (turn.complete event count, dominant agent name).
async fn fetch_turn_complete_count_and_agent(
    client: &reqwest::Client,
    session_id: &str,
) -> (usize, String) {
    let url = format!("{BASE}/api/sessions/{session_id}/events");
    let events: Vec<Value> = match client.get(&url).send().await {
        Ok(resp) => match resp.json::<Value>().await {
            Ok(body) => body
                .as_array()
                .cloned()
                .or_else(|| body.get("events").and_then(|v| v.as_array()).cloned())
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        },
        Err(_) => Vec::new(),
    };

    let count = events
        .iter()
        .filter(|e| e.get("subtype").and_then(|v| v.as_str()) == Some("system.turn.complete"))
        .count();

    // Dominant agent
    let mut agent_counts: HashMap<String, usize> = HashMap::new();
    for e in &events {
        if let Some(a) = e.get("agent").and_then(|v| v.as_str()) {
            *agent_counts.entry(a.to_string()).or_insert(0) += 1;
        }
    }
    let agent = agent_counts
        .into_iter()
        .max_by_key(|(_, n)| *n)
        .map(|(a, _)| a)
        .unwrap_or_else(|| "?".to_string());

    (count, agent)
}

async fn fetch_patterns(client: &reqwest::Client, session_id: &str) -> Vec<Value> {
    let url = format!("{BASE}/api/sessions/{session_id}/patterns");
    match client.get(&url).send().await {
        Ok(resp) => match resp.json::<Value>().await {
            Ok(body) => body
                .as_array()
                .cloned()
                .or_else(|| body.get("patterns").and_then(|v| v.as_array()).cloned())
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        },
        Err(_) => Vec::new(),
    }
}

/// Return list of legibility issues for a sentence. Empty list = legible.
fn legibility_issues(summary: &str, metadata: &Value) -> Vec<&'static str> {
    let mut issues = Vec::new();

    let subject = metadata.get("subject").and_then(|v| v.as_str()).unwrap_or("");
    let verb = metadata.get("verb").and_then(|v| v.as_str()).unwrap_or("");
    let object = metadata.get("object").and_then(|v| v.as_str()).unwrap_or("");
    let subordinates_empty = metadata
        .get("subordinates")
        .and_then(|v| v.as_array())
        .map(|a| a.is_empty())
        .unwrap_or(true);

    if subject.is_empty() {
        issues.push("missing subject");
    }
    if verb.is_empty() {
        issues.push("missing verb");
    }
    if object.is_empty() && subordinates_empty {
        issues.push("missing both object and subordinates (too thin)");
    }
    if summary.chars().count() < MIN_SUMMARY_CHARS {
        issues.push("summary too short");
    }

    issues
}

fn short(id: &str) -> &str {
    if id.len() >= 12 { &id[..12] } else { id }
}

