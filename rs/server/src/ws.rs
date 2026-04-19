//! WebSocket handler — live event streaming to browser clients.
//!
//! On connect: sends `initial_state` with WireRecords from projection cache.
//! Progress events are excluded from initial_state (ephemeral).
//! Then streams live broadcast messages as they arrive.

use std::collections::HashMap;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use serde::Serialize;
use serde_json::json;

use open_story_views::wire_record::WireRecord;

use open_story_patterns::PatternEvent;
use open_story_store::ingest::to_wire_record;
use open_story_store::projection::{filter_matches, FILTER_NAMES};

use crate::logging::log_event;
use crate::state::{AppState, SharedState};

/// Session label data for the initial_state message.
#[derive(Debug, Clone, Serialize)]
pub struct SessionLabel {
    pub label: Option<String>,
    pub branch: Option<String>,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
}

// MAX_INITIAL_RECORDS now comes from config.max_initial_records

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Initial state payload components.
pub struct InitialState {
    pub records: Vec<WireRecord>,
    pub filter_counts: HashMap<String, HashMap<String, usize>>,
    pub patterns: Vec<PatternEvent>,
    pub session_labels: HashMap<String, SessionLabel>,
}

/// Build initial_state from projection cache.
///
/// Returns all components needed for the WS handshake message.
/// Excludes progress/ephemeral records. Caps at MAX_INITIAL_RECORDS (most recent).
/// Public for testing.
pub fn build_initial_state(state: &AppState) -> InitialState {
    let mut all_records: Vec<WireRecord> = Vec::new();
    let mut all_filter_counts: HashMap<String, HashMap<String, usize>> = HashMap::new();
    let mut all_patterns: Vec<PatternEvent> = Vec::new();
    let mut session_labels: HashMap<String, SessionLabel> = HashMap::new();

    for entry in state.store.projections.iter() {
        let sid = entry.key();
        let proj = entry.value();
        all_filter_counts.insert(sid.clone(), proj.filter_counts().clone());

        session_labels.insert(sid.clone(), SessionLabel {
            label: proj.label().map(|s| s.to_string()),
            branch: proj.branch().map(|s| s.to_string()),
            total_input_tokens: proj.total_input_tokens(),
            total_output_tokens: proj.total_output_tokens(),
        });

        for vr in proj.timeline_rows() {
            all_records.push(to_wire_record(vr, proj));
        }
    }

    // Collect all detected patterns from in-memory cache (authoritative for live state)
    for entry in state.store.detected_patterns.iter() {
        all_patterns.extend(entry.value().iter().cloned());
    }

    // Sort by timestamp, then cap at most recent
    all_records.sort_by(|a, b| a.record.timestamp.cmp(&b.record.timestamp));
    let total = all_records.len();
    let max_records = state.config.max_initial_records;
    if all_records.len() > max_records {
        all_records = all_records.split_off(total - max_records);

        // Recompute filter_counts from the capped set — projection counts
        // reflect the full history, but the UI should only see counts for
        // records actually delivered.
        all_filter_counts.clear();
        for wr in &all_records {
            let session_counts = all_filter_counts
                .entry(wr.record.session_id.clone())
                .or_default();
            for name in FILTER_NAMES {
                if filter_matches(name, &wr.record) {
                    *session_counts.entry(name.to_string()).or_insert(0) += 1;
                }
            }
        }
    }

    log_event(
        "ws",
        &format!(
            "building initial_state ({} wire_records, {} total from {} sessions, {} patterns, {} labels)",
            all_records.len(),
            total,
            state.store.projections.len(),
            all_patterns.len(),
            session_labels.len(),
        ),
    );

    InitialState {
        records: all_records,
        filter_counts: all_filter_counts,
        patterns: all_patterns,
        session_labels,
    }
}

async fn handle_socket(mut socket: WebSocket, state: SharedState) {
    log_event("ws", "client \x1b[32mconnected\x1b[0m");

    let mut events_forwarded: u64 = 0;

    // Send initial_state from projection cache
    let init = {
        let s = state.read().await;
        build_initial_state(&s)
    };

    let initial_msg = json!({
        "kind": "initial_state",
        "records": init.records,
        "filter_counts": init.filter_counts,
        "patterns": init.patterns,
        "session_labels": init.session_labels,
    });

    if socket
        .send(Message::Text(
            serde_json::to_string(&initial_msg)
                .unwrap_or_default()
                .into(),
        ))
        .await
        .is_err()
    {
        log_event("ws", "client \x1b[2mdisconnected\x1b[0m");
        return;
    }

    // Subscribe to broadcast channel
    let mut rx = {
        let s = state.read().await;
        s.broadcast_tx.subscribe()
    };

    // Forward broadcast messages to this WebSocket
    loop {
        tokio::select! {
            // Broadcast message from hooks
            result = rx.recv() => {
                match result {
                    Ok(msg) => {
                        events_forwarded += 1;
                        crate::metrics::record_ws_message_sent();
                        let text = serde_json::to_string(&msg).unwrap_or_default();
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        log_event("ws", &format!("\x1b[33mlagged — skipped {n} messages\x1b[0m"));
                        continue;
                    }
                    Err(_) => break,
                }
            }
            // Client message (keep-alive or disconnect)
            msg = socket.recv() => {
                match msg {
                    Some(Ok(_)) => {} // Ignore client messages
                    _ => break,       // Disconnect
                }
            }
        }
    }

    log_event(
        "ws",
        &format!("client \x1b[2mdisconnected\x1b[0m ({events_forwarded} events forwarded)"),
    );
}

// ── Audit walk #8 (2026-04-15) — build_initial_state coverage ─────
//
// Whole file had zero inline tests before this commit. build_initial_state
// is the entire WS handshake — every connecting client gets a snapshot
// derived from the current projection state. Bugs here are silent
// divergence between server truth and what the UI renders on connect.
//
// See docs/research/architecture-audit/WS_LAYER_WALK.md for the full
// findings (Lagged handling, cap-without-notice, swallowed serde).

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::state::AppState;
    use open_story_bus::noop_bus::NoopBus;
    use open_story_store::projection::SessionProjection;
    use open_story_store::state::StoreState;
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::broadcast;

    fn fresh_app_state(tmp: &TempDir) -> AppState {
        let store = StoreState::new(tmp.path()).unwrap();
        let (broadcast_tx, _) = broadcast::channel(256);
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&watch_dir).unwrap();
        AppState {
            store,
            transcript_states: HashMap::new(),
            broadcast_tx,
            bus: Arc::new(NoopBus),
            config: Config::default(),
            watch_dir,
        }
    }

    fn user_event(id: &str, sid: &str, time: &str, text: &str) -> serde_json::Value {
        serde_json::json!({
            "specversion": "1.0",
            "id": id,
            "type": "io.arc.event",
            "subtype": "message.user.prompt",
            "source": "arc://test",
            "time": time,
            "datacontenttype": "application/json",
            "agent": "claude-code",
            "data": {
                "raw": {},
                "seq": 1,
                "session_id": sid,
                "agent_payload": {
                    "_variant": "claude-code",
                    "meta": {"agent": "claude-code"},
                    "text": text,
                }
            }
        })
    }

    #[test]
    fn initial_state_is_empty_for_empty_projection_map() {
        let tmp = TempDir::new().unwrap();
        let state = fresh_app_state(&tmp);
        let init = build_initial_state(&state);
        assert!(init.records.is_empty());
        assert!(init.filter_counts.is_empty());
        assert!(init.session_labels.is_empty());
    }

    #[test]
    fn initial_state_includes_records_from_each_session() {
        let tmp = TempDir::new().unwrap();
        let state = fresh_app_state(&tmp);

        let mut p1 = SessionProjection::new("sess-1");
        p1.append(&user_event("evt-a", "sess-1", "2026-04-15T00:00:00Z", "first"));
        state.store.projections.insert("sess-1".to_string(), p1);

        let mut p2 = SessionProjection::new("sess-2");
        p2.append(&user_event("evt-b", "sess-2", "2026-04-15T00:00:01Z", "second"));
        state.store.projections.insert("sess-2".to_string(), p2);

        let init = build_initial_state(&state);
        assert_eq!(init.records.len(), 2);
        assert_eq!(init.session_labels.len(), 2);
    }

    #[test]
    fn initial_state_sorts_records_by_timestamp() {
        let tmp = TempDir::new().unwrap();
        let state = fresh_app_state(&tmp);
        let mut p = SessionProjection::new("sess-1");
        p.append(&user_event("evt-late", "sess-1", "2026-04-15T00:00:02Z", "later"));
        p.append(&user_event("evt-early", "sess-1", "2026-04-15T00:00:00Z", "earlier"));
        p.append(&user_event("evt-mid", "sess-1", "2026-04-15T00:00:01Z", "middle"));
        state.store.projections.insert("sess-1".to_string(), p);

        let init = build_initial_state(&state);
        let times: Vec<_> = init.records.iter().map(|r| r.record.timestamp.as_str()).collect();
        assert_eq!(
            times,
            vec!["2026-04-15T00:00:00Z", "2026-04-15T00:00:01Z", "2026-04-15T00:00:02Z"]
        );
    }

    #[test]
    fn initial_state_caps_to_max_records_keeping_most_recent() {
        // F-2 characterization: the cap silently drops oldest records.
        // No notification to the client that older records exist.
        let tmp = TempDir::new().unwrap();
        let mut state = fresh_app_state(&tmp);
        state.config.max_initial_records = 2;

        let mut p = SessionProjection::new("sess-1");
        for i in 0..5 {
            p.append(&user_event(
                &format!("evt-{i}"),
                "sess-1",
                &format!("2026-04-15T00:00:0{i}Z"),
                &format!("msg {i}"),
            ));
        }
        state.store.projections.insert("sess-1".to_string(), p);

        let init = build_initial_state(&state);
        assert_eq!(init.records.len(), 2, "capped to max_initial_records");
        // Most recent two
        let times: Vec<_> = init.records.iter().map(|r| r.record.timestamp.as_str()).collect();
        assert_eq!(times, vec!["2026-04-15T00:00:03Z", "2026-04-15T00:00:04Z"]);
    }

    #[test]
    fn initial_state_recomputes_filter_counts_from_capped_set() {
        // The full projection has counts for ALL events; after capping,
        // filter_counts must reflect only what's actually delivered.
        // Otherwise the UI sidebar shows counts that don't match the
        // visible records.
        let tmp = TempDir::new().unwrap();
        let mut state = fresh_app_state(&tmp);
        state.config.max_initial_records = 2;

        let mut p = SessionProjection::new("sess-1");
        for i in 0..5 {
            p.append(&user_event(
                &format!("evt-{i}"),
                "sess-1",
                &format!("2026-04-15T00:00:0{i}Z"),
                &format!("user msg {i}"),
            ));
        }
        // projection's own filter count for "user" is 5
        let pre_cap_user = p.filter_counts().get("user").copied().unwrap_or(0);
        assert_eq!(pre_cap_user, 5);
        state.store.projections.insert("sess-1".to_string(), p);

        let init = build_initial_state(&state);
        // After cap, filter_counts["user"] for sess-1 should be 2, not 5
        let counts = init.filter_counts.get("sess-1").expect("sess-1 counts");
        assert_eq!(
            counts.get("user").copied().unwrap_or(0),
            2,
            "filter_counts must be recomputed from capped record set"
        );
    }

    #[test]
    fn initial_state_session_labels_carry_label_branch_and_token_totals() {
        let tmp = TempDir::new().unwrap();
        let state = fresh_app_state(&tmp);
        let mut p = SessionProjection::new("sess-x");
        p.append(&user_event("evt-1", "sess-x", "2026-04-15T00:00:00Z", "implement feature thing"));
        state.store.projections.insert("sess-x".to_string(), p);

        let init = build_initial_state(&state);
        let label = init.session_labels.get("sess-x").expect("sess-x label");
        assert!(label.label.is_some(), "label populated from first prompt");
        assert!(label.label.as_ref().unwrap().contains("implement"));
        // No token events yet, so totals are 0
        assert_eq!(label.total_input_tokens, 0);
        assert_eq!(label.total_output_tokens, 0);
    }

    #[test]
    fn initial_state_excludes_progress_events_via_projection_filter() {
        // Per the doc comment at ws.rs:53: progress/ephemeral records
        // are excluded. SessionProjection doesn't store ephemeral
        // events in timeline_rows — characterizing the contract.
        let tmp = TempDir::new().unwrap();
        let state = fresh_app_state(&tmp);
        let mut p = SessionProjection::new("sess-1");
        p.append(&user_event("evt-real", "sess-1", "2026-04-15T00:00:00Z", "real"));
        state.store.projections.insert("sess-1".to_string(), p);

        let init = build_initial_state(&state);
        // Only the durable user prompt comes through.
        assert_eq!(init.records.len(), 1);
    }
}

