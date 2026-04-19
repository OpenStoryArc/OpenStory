//! Golden replay snapshot tests.
//!
//! Freezes the current `ingest_events` behavior against three representative
//! fixtures so the Actor 4 decomposition refactor has a load-bearing
//! safety net. After Phase 1 lands, re-running these tests against the
//! new actor pipeline must produce byte-identical snapshots (or any diff
//! must be consciously reviewed and the snapshot updated).
//!
//! **Updating snapshots:** set `UPDATE_GOLDEN=1` when running, e.g.
//!   `UPDATE_GOLDEN=1 cargo test -p open-story --test test_golden_replay`
//!
//! Without that env var, a mismatch fails the test and prints a diff.
//!
//! This is commit 0b of the TDD plan at
//! `/Users/maxglassie/.claude/plans/can-we-plan-a-cuddly-moler.md`.

mod helpers;

use std::path::{Path, PathBuf};

use open_story::cloud_event::CloudEvent;
use open_story::translate::{translate_line, TranscriptFormat, TranscriptState};
use open_story_core::translate_hermes::{is_hermes_format, translate_hermes_line};
use open_story_core::translate_pi::{is_pi_mono_format, translate_pi_line};
use open_story::server::ingest_events;
use serde_json::{json, Value};

use helpers::test_state;

fn fixtures_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn snapshots_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}

/// Translate a parsed JSONL line using the format detected for the fixture.
fn translate_for_format(
    format: &TranscriptFormat,
    line: &Value,
    state: &mut TranscriptState,
) -> Vec<CloudEvent> {
    match format {
        TranscriptFormat::Hermes => translate_hermes_line(line, state),
        TranscriptFormat::PiMono => translate_pi_line(line, state),
        _ => translate_line(line, state),
    }
}

/// Same detection order as `rs/core/src/reader.rs:101-108`.
fn detect_format(first_line: &Value) -> TranscriptFormat {
    if is_hermes_format(first_line) {
        TranscriptFormat::Hermes
    } else if is_pi_mono_format(first_line) {
        TranscriptFormat::PiMono
    } else {
        TranscriptFormat::ClaudeCode
    }
}

/// Read a fixture, translate every line, feed results through `ingest_events`,
/// and return a canonical JSON snapshot of the resulting state. Each call
/// uses its own local monotonic counter — keeps snapshots deterministic
/// when multiple golden tests run in parallel.
async fn capture_snapshot(fixture_path: &Path, session_id: &str) -> Value {
    let mut next_seq: u64 = 0;

    let tmp = tempfile::tempdir().expect("create temp dir");
    let state_arc = test_state(&tmp);

    let text = std::fs::read_to_string(fixture_path)
        .unwrap_or_else(|e| panic!("read fixture {fixture_path:?}: {e}"));

    // Detect format from the first non-empty line.
    let first_line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let first_value: Value = serde_json::from_str(first_line)
        .unwrap_or_else(|e| panic!("parse first line of {fixture_path:?}: {e}"));
    let format = detect_format(&first_value);

    let mut transcript_state = TranscriptState::new(session_id.to_string());
    let mut all_broadcast_messages: Vec<Value> = Vec::new();

    for (line_num, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let parsed: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => panic!("parse line {} of {:?}: {}", line_num + 1, fixture_path, e),
        };

        let events = translate_for_format(&format, &parsed, &mut transcript_state);
        if events.is_empty() {
            continue;
        }

        // Assign monotonic seq so snapshot ordering is stable.
        let events: Vec<CloudEvent> = events
            .into_iter()
            .map(|mut e| {
                e.data.seq = next_seq;
                next_seq += 1;
                e
            })
            .collect();

        let mut state = state_arc.write().await;
        let result = ingest_events(&mut state, session_id, &events, None).await;
        for m in result.changes {
            if let Ok(v) = serde_json::to_value(&m) {
                all_broadcast_messages.push(v);
            }
        }
    }

    // Canonicalize post-state.
    let state = state_arc.read().await;
    let sessions = state
        .store
        .event_store
        .list_sessions()
        .await
        .unwrap_or_default();
    let mut session_rows: Vec<Value> = sessions
        .iter()
        .map(|r| {
            json!({
                "id": r.id,
                "project_id": r.project_id,
                "project_name": r.project_name,
                "label": r.label,
                "custom_label": r.custom_label,
                "branch": r.branch,
                "event_count": r.event_count,
                "first_event": r.first_event,
                "last_event": r.last_event,
            })
        })
        .collect();
    session_rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));

    let mut events: Vec<Value> = state
        .store
        .event_store
        .session_events(session_id)
        .await
        .unwrap_or_default();
    // Stable order: by `seq` then `id`.
    events.sort_by(|a, b| {
        let sa = a.get("data").and_then(|d| d.get("seq")).and_then(|v| v.as_u64()).unwrap_or(0);
        let sb = b.get("data").and_then(|d| d.get("seq")).and_then(|v| v.as_u64()).unwrap_or(0);
        sa.cmp(&sb).then_with(|| {
            a.get("id").and_then(|v| v.as_str()).cmp(&b.get("id").and_then(|v| v.as_str()))
        })
    });

    let projection_summary = state
        .store
        .projections
        .get(session_id)
        .map(|p| {
            json!({
                "label": p.label(),
                "branch": p.branch(),
                "total_input_tokens": p.total_input_tokens(),
                "total_output_tokens": p.total_output_tokens(),
                "event_count": p.event_count(),
            })
        })
        .unwrap_or(Value::Null);

    let full_payloads_keys: Vec<String> = state
        .store
        .full_payloads
        .get(session_id)
        .map(|m| {
            let mut v: Vec<String> = m.keys().cloned().collect();
            v.sort();
            v
        })
        .unwrap_or_default();

    json!({
        "sessions": session_rows,
        "events": events,
        "projection": projection_summary,
        "full_payloads_keys": full_payloads_keys,
        "broadcast_messages": all_broadcast_messages,
    })
}

/// Replace every UUID-shaped substring in the snapshot with a stable
/// placeholder (`uuid-0`, `uuid-1`, ...) in order of first appearance.
/// Preserves referential identity — two occurrences of the same UUID
/// get the same placeholder.
///
/// The ingest pipeline synthesizes UUIDs (e.g., for decomposed content
/// blocks and `system.turn.complete` markers) with non-deterministic v4
/// generation. Snapshotting raw UUIDs would fail between runs. We
/// capture structural behavior instead — "the Nth event in the stream
/// has UUID X" — and normalize X.
fn normalize_uuids(text: &str) -> String {
    use std::collections::HashMap;
    // UUID v4 pattern: 8-4-4-4-12 lowercase hex.
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut map: HashMap<String, String> = HashMap::new();
    let mut i = 0;
    while i < bytes.len() {
        if i + 36 <= bytes.len() && looks_like_uuid(&bytes[i..i + 36]) {
            let uuid = std::str::from_utf8(&bytes[i..i + 36]).unwrap().to_string();
            let next_idx = map.len();
            let placeholder = map
                .entry(uuid)
                .or_insert_with(|| format!("uuid-{next_idx}"))
                .clone();
            out.push_str(&placeholder);
            i += 36;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    out
}

fn looks_like_uuid(s: &[u8]) -> bool {
    if s.len() != 36 {
        return false;
    }
    for (i, b) in s.iter().enumerate() {
        let c = *b as char;
        match i {
            8 | 13 | 18 | 23 => {
                if c != '-' {
                    return false;
                }
            }
            _ => {
                if !c.is_ascii_hexdigit() || c.is_ascii_uppercase() {
                    return false;
                }
            }
        }
    }
    true
}

fn canonical_string(v: &Value) -> String {
    let raw = serde_json::to_string_pretty(v).expect("pretty-print snapshot");
    normalize_uuids(&raw)
}

/// Compare `actual` against the snapshot file; if `UPDATE_GOLDEN=1`, rewrite it.
fn assert_or_update_snapshot(name: &str, actual: &Value) {
    let path = snapshots_root().join(format!("{name}.golden.json"));
    let actual_text = canonical_string(actual) + "\n";

    if std::env::var("UPDATE_GOLDEN").ok().as_deref() == Some("1") {
        std::fs::create_dir_all(snapshots_root()).expect("create snapshots dir");
        std::fs::write(&path, &actual_text).unwrap_or_else(|e| panic!("write {path:?}: {e}"));
        eprintln!("UPDATE_GOLDEN=1 — wrote {path:?}");
        return;
    }

    let expected = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => panic!(
            "snapshot missing: {path:?}\n\
             Run with UPDATE_GOLDEN=1 to create it, then review the diff before committing."
        ),
    };

    if expected != actual_text {
        // Compact diff: show the first differing lines so the panic message
        // is actually readable.
        let exp_lines: Vec<&str> = expected.lines().collect();
        let act_lines: Vec<&str> = actual_text.lines().collect();
        let max = exp_lines.len().max(act_lines.len());
        let mut diff = String::new();
        let mut shown = 0;
        for i in 0..max {
            let e = exp_lines.get(i).copied().unwrap_or("<eof>");
            let a = act_lines.get(i).copied().unwrap_or("<eof>");
            if e != a {
                diff.push_str(&format!("line {}:\n- {}\n+ {}\n", i + 1, e, a));
                shown += 1;
                if shown >= 5 {
                    diff.push_str("... (truncated)\n");
                    break;
                }
            }
        }
        panic!(
            "snapshot {path:?} does not match actual.\n{diff}\n\
             If the new state is intentional, re-run with UPDATE_GOLDEN=1 and review the file diff."
        );
    }
}

// ── Tests ──

/// pi-mono happy-path session (10 lines, decomposition + synthetic turn.complete).
#[tokio::test]
async fn golden_pi_mono_session() {
    let actual = capture_snapshot(
        &fixtures_root().join("pi_mono_session.jsonl"),
        "pi_mono_session",
    )
    .await;
    assert_or_update_snapshot("pi_mono_session", &actual);
}

/// pi-mono scenario with parallel tool calls in a single message (decomposition
/// must split them into 2 separate tool_use events).
#[tokio::test]
async fn golden_pi_mono_scenario_07_multi_tool() {
    let actual = capture_snapshot(
        &fixtures_root().join("pi_mono/scenario_07_multi_tool.jsonl"),
        "scenario_07_multi_tool",
    )
    .await;
    assert_or_update_snapshot("pi_mono_scenario_07_multi_tool", &actual);
}

/// Claude Code synthetic session — covers the primary Claude translation path.
#[tokio::test]
async fn golden_synthetic_claude() {
    let actual = capture_snapshot(
        &fixtures_root().join("synthetic.jsonl"),
        "synthetic",
    )
    .await;
    assert_or_update_snapshot("synthetic_claude", &actual);
}
