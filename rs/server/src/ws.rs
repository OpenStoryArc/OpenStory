//! WebSocket handler — live event streaming to browser clients.
//!
//! On connect: sends a small `initial_state` snapshot (session labels +
//! recent patterns) bounded by `watch_backfill_hours`. Records are NOT
//! shipped on the handshake — the UI lazy-loads them via the REST
//! `/api/sessions/{id}/records` endpoint when the user opens a session.
//! After the handshake, this socket forwards live `BroadcastMessage`s
//! from `state.broadcast_tx` until either side closes.

use std::collections::HashMap;

use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use serde::Serialize;
use serde_json::json;

use open_story_patterns::PatternEvent;

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

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<SharedState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Initial state payload — sidebar-only data.
///
/// Records are intentionally absent: shipping every session's full
/// timeline on connect produced a 39 MB handshake on real boxes, blowing
/// past the 1 MB frame limit common in WS clients and freezing the UI.
/// The UI now fetches per-session records via REST on demand.
pub struct InitialState {
    pub patterns: Vec<PatternEvent>,
    pub session_labels: HashMap<String, SessionLabel>,
}

/// Build initial_state from projection cache, bounded by recency.
///
/// Only sessions whose most-recent `timeline_rows()` entry falls within
/// `config.watch_backfill_hours` of "now" are included. This caps the
/// handshake at a few KB even when 10k historical sessions are present.
/// Older sessions are still discoverable via the REST `/api/sessions`
/// endpoint (which the sidebar queries directly).
pub fn build_initial_state(state: &AppState) -> InitialState {
    let mut session_labels: HashMap<String, SessionLabel> = HashMap::new();
    let mut all_patterns: Vec<PatternEvent> = Vec::new();
    let mut included_sessions: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    let cutoff = recency_cutoff(state.config.watch_backfill_hours);

    for entry in state.store.projections.iter() {
        let sid = entry.key();
        let proj = entry.value();

        if !is_recent(proj, cutoff.as_deref()) {
            continue;
        }
        included_sessions.insert(sid.clone());

        session_labels.insert(
            sid.clone(),
            SessionLabel {
                label: proj.label().map(|s| s.to_string()),
                branch: proj.branch().map(|s| s.to_string()),
                total_input_tokens: proj.total_input_tokens(),
                total_output_tokens: proj.total_output_tokens(),
            },
        );
    }

    for entry in state.store.detected_patterns.iter() {
        if !included_sessions.contains(entry.key()) {
            continue;
        }
        all_patterns.extend(entry.value().iter().cloned());
    }

    log_event(
        "ws",
        &format!(
            "building initial_state ({} session_labels, {} patterns; recency window {}h, total projections {})",
            session_labels.len(),
            all_patterns.len(),
            state.config.watch_backfill_hours,
            state.store.projections.len(),
        ),
    );

    InitialState {
        patterns: all_patterns,
        session_labels,
    }
}

/// Compute the RFC3339 cutoff string for "now - hours". Returns `None`
/// when `hours == 0` (caller treats that as "no recency filter").
fn recency_cutoff(hours: u64) -> Option<String> {
    if hours == 0 {
        return None;
    }
    let now = chrono::Utc::now();
    let cutoff = now - chrono::Duration::hours(hours as i64);
    Some(cutoff.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// True if the projection's most recent timeline row is at or after the
/// cutoff. When `cutoff` is `None`, every projection is recent.
/// Empty projections are treated as recent (they were just created and
/// haven't been written to yet).
fn is_recent(
    proj: &open_story_store::projection::SessionProjection,
    cutoff: Option<&str>,
) -> bool {
    let Some(cutoff) = cutoff else {
        return true;
    };
    let rows = proj.timeline_rows();
    if rows.is_empty() {
        return true;
    }
    let max_ts = rows
        .iter()
        .map(|vr| vr.timestamp.as_str())
        .max()
        .unwrap_or("");
    max_ts >= cutoff
}

async fn handle_socket(mut socket: WebSocket, state: SharedState) {
    log_event("ws", "client \x1b[32mconnected\x1b[0m");

    let mut events_forwarded: u64 = 0;

    let init = {
        let s = state.read().await;
        build_initial_state(&s)
    };

    let initial_msg = json!({
        "kind": "initial_state",
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

    let mut rx = {
        let s = state.read().await;
        s.broadcast_tx.subscribe()
    };

    loop {
        tokio::select! {
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
        let mut config = Config::default();
        // Tests run with a fresh "now"; default 24h would only cover events
        // dated in the last day. Most fixtures use timestamps in 2026, so
        // disable the recency filter (hours = 0 → include everything) unless
        // a test sets it explicitly.
        config.watch_backfill_hours = 0;
        AppState {
            store,
            transcript_states: HashMap::new(),
            broadcast_tx,
            bus: Arc::new(NoopBus),
            config,
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
        assert!(init.patterns.is_empty());
        assert!(init.session_labels.is_empty());
    }

    #[test]
    fn initial_state_includes_session_labels_from_each_session() {
        let tmp = TempDir::new().unwrap();
        let state = fresh_app_state(&tmp);

        let mut p1 = SessionProjection::new("sess-1");
        p1.append(&user_event("evt-a", "sess-1", "2026-04-15T00:00:00Z", "first"));
        state.store.projections.insert("sess-1".to_string(), p1);

        let mut p2 = SessionProjection::new("sess-2");
        p2.append(&user_event("evt-b", "sess-2", "2026-04-15T00:00:01Z", "second"));
        state.store.projections.insert("sess-2".to_string(), p2);

        let init = build_initial_state(&state);
        assert_eq!(init.session_labels.len(), 2);
        assert!(init.session_labels.contains_key("sess-1"));
        assert!(init.session_labels.contains_key("sess-2"));
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
        assert_eq!(label.total_input_tokens, 0);
        assert_eq!(label.total_output_tokens, 0);
    }

    #[test]
    fn initial_state_omits_sessions_outside_recency_window() {
        // With a 1-hour window relative to "now", a 2026-dated session
        // (well before now in test time) is filtered out.
        let tmp = TempDir::new().unwrap();
        let mut state = fresh_app_state(&tmp);
        state.config.watch_backfill_hours = 1;

        let mut old = SessionProjection::new("old-sess");
        old.append(&user_event(
            "evt-old",
            "old-sess",
            "2026-04-15T00:00:00Z",
            "ancient",
        ));
        state.store.projections.insert("old-sess".to_string(), old);

        let init = build_initial_state(&state);
        assert!(
            !init.session_labels.contains_key("old-sess"),
            "session outside recency window must not appear in handshake"
        );
    }

    #[test]
    fn initial_state_includes_sessions_with_zero_hour_window_disabled() {
        // hours=0 disables the recency filter (every session ships).
        let tmp = TempDir::new().unwrap();
        let mut state = fresh_app_state(&tmp);
        state.config.watch_backfill_hours = 0;

        let mut p = SessionProjection::new("any-sess");
        p.append(&user_event(
            "evt-1",
            "any-sess",
            "1999-01-01T00:00:00Z",
            "very old",
        ));
        state.store.projections.insert("any-sess".to_string(), p);

        let init = build_initial_state(&state);
        assert!(init.session_labels.contains_key("any-sess"));
    }

    #[test]
    fn initial_state_handshake_payload_is_small_for_many_sessions() {
        // The whole point of the redesign: 1000 sessions worth of labels
        // serializes to well under 100 KB. (Pre-fix produced ~40 MB on
        // 327 sessions because records were included.)
        let tmp = TempDir::new().unwrap();
        let state = fresh_app_state(&tmp);

        for i in 0..1000 {
            let sid = format!("sess-{i}");
            let mut p = SessionProjection::new(&sid);
            p.append(&user_event(
                &format!("evt-{i}"),
                &sid,
                "2026-04-15T00:00:00Z",
                "label content",
            ));
            state.store.projections.insert(sid, p);
        }

        let init = build_initial_state(&state);
        let json = serde_json::to_string(&serde_json::json!({
            "kind": "initial_state",
            "patterns": init.patterns,
            "session_labels": init.session_labels,
        }))
        .unwrap();
        assert!(
            json.len() < 200_000,
            "handshake should be < 200KB even with 1000 sessions; was {} bytes",
            json.len()
        );
    }
}
