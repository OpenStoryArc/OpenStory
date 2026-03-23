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
    pub agent_labels: HashMap<String, String>,
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

    for (sid, proj) in &state.store.projections {
        // Collect filter counts from each session's projection
        all_filter_counts.insert(sid.clone(), proj.filter_counts().clone());

        // Collect session labels (always include for token counts)
        session_labels.insert(sid.clone(), SessionLabel {
            label: proj.label().map(|s| s.to_string()),
            branch: proj.branch().map(|s| s.to_string()),
            total_input_tokens: proj.total_input_tokens(),
            total_output_tokens: proj.total_output_tokens(),
        });

        // Convert timeline rows to WireRecords (these are already durable — projection
        // doesn't store ephemeral records in timeline_rows)
        for vr in proj.timeline_rows() {
            all_records.push(to_wire_record(vr, proj));
        }
    }

    // Collect all detected patterns from in-memory cache (authoritative for live state)
    for patterns in state.store.detected_patterns.values() {
        all_patterns.extend(patterns.iter().cloned());
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
        agent_labels: state.store.agent_labels.clone(),
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
        "agent_labels": init.agent_labels,
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
