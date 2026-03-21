//! Slim POST /hooks endpoint — reads transcript on hook trigger.
//!
//! Claude Code hooks fire at precise moments (PreToolUse, PostToolUse, Stop, etc.)
//! and POST a payload with session_id + transcript path. This handler reads new
//! lines from the transcript file using shared TranscriptState and ingests them
//! as CloudEvents — giving instant real-time updates.
//!
//! The watcher serves as the background discovery/backfill channel. Both produce
//! the same `io.arc.transcript.*` CloudEvents. Deduplication happens in
//! `ingest_events()` via the CloudEvent `id` field.

use std::path::PathBuf;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};
use walkdir::WalkDir;

use open_story_bus::IngestBatch;

use open_story_core::reader::read_new_lines;
use open_story_core::translate::TranscriptState;
use open_story_core::paths::{project_id_from_path, session_id_from_path};

use crate::ingest::ingest_events;
use crate::logging::{event_type_summary, log_event, short_id};
use crate::state::{AppState, SharedState};

/// Resolve the transcript path for a hook request using a 3-tier strategy:
///
/// 1. Use `transcript_path` from payload if present and file exists
/// 2. Search `transcript_states` for a path matching the session_id
/// 3. Walk `watch_dir` for `{session_id}.jsonl`
fn resolve_transcript_path(
    body: &Value,
    session_id: &str,
    state: &AppState,
) -> Option<PathBuf> {
    // Tier 1: explicit path from payload
    if let Some(p) = body.get("transcript_path").and_then(|v| v.as_str()) {
        let path = PathBuf::from(p);
        if path.exists() {
            return Some(path);
        }
    }

    // Tier 2: search transcript_states for matching session_id
    if !session_id.is_empty() {
        for (path, ts) in &state.transcript_states {
            if ts.session_id == session_id && path.exists() {
                return Some(path.clone());
            }
        }
    }

    // Tier 3: walk watch_dir for {session_id}.jsonl
    if !session_id.is_empty() {
        let target_filename = format!("{}.jsonl", session_id);
        let canonical_watch = state.watch_dir.canonicalize().ok();
        for entry in WalkDir::new(&state.watch_dir)
            .follow_links(false) // Don't follow symlinks — prevents symlink traversal
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_type().is_file() {
                if let Some(name) = entry.file_name().to_str() {
                    if name == target_filename {
                        // Verify resolved path stays within watch_dir
                        let candidate = entry.into_path();
                        if let (Ok(canonical_path), Some(ref watch_base)) =
                            (candidate.canonicalize(), &canonical_watch)
                        {
                            if canonical_path.starts_with(watch_base) {
                                return Some(candidate);
                            }
                        }
                    }
                }
            }
        }
    }

    None
}

/// Receive a hook POST from Claude Code.
///
/// Expected payload fields (all optional — we degrade gracefully):
/// - `session_id`: the Claude Code session ID
/// - `transcript_path`: absolute path to the session's JSONL transcript file
///
/// Uses 3-tier resolution to find the transcript file. If no transcript can be
/// found, returns ACCEPTED with no action — the watcher will pick it up eventually.
pub async fn receive_hook(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    crate::metrics::record_hook_received();

    let session_id_raw = body
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut s = state.write().await;

    let transcript_path = match resolve_transcript_path(&body, &session_id_raw, &s) {
        Some(p) => p,
        None => {
            return (
                StatusCode::ACCEPTED,
                Json(json!({"status": "no_transcript"})),
            );
        }
    };

    // Derive session_id from path if not provided in payload
    let session_id = if session_id_raw.is_empty() {
        session_id_from_path(&transcript_path)
    } else {
        session_id_raw
    };

    // Get or create TranscriptState for this file
    let ts = s
        .transcript_states
        .entry(transcript_path.clone())
        .or_insert_with(|| TranscriptState::new(session_id.clone()));

    let events = match read_new_lines(&transcript_path, ts) {
        Ok(evts) => evts,
        Err(_) => {
            return (
                StatusCode::ACCEPTED,
                Json(json!({"status": "read_error"})),
            );
        }
    };

    // Derive project_id from transcript path relative to watch_dir
    let project_id = project_id_from_path(&transcript_path, &s.watch_dir);

    let summary = event_type_summary(&events);
    let event_count = events.len();

    if s.bus.is_active() {
        // Bus mode: publish to NATS, consumer will call ingest_events()
        let batch = IngestBatch {
            session_id: session_id.clone(),
            project_id: project_id.unwrap_or_default(),
            events: events.to_vec(),
        };
        let subject = format!("events.session.{session_id}");
        let bus = s.bus.clone();
        // Drop the write lock before async publish
        drop(s);

        let count = match bus.publish(&subject, &batch).await {
            Ok(_) => event_count,
            Err(e) => {
                eprintln!("Hook bus publish error: {e}");
                return (
                    StatusCode::ACCEPTED,
                    Json(json!({"status": "bus_error"})),
                );
            }
        };

        if count > 0 {
            let hook_name = body
                .get("hook_event_name")
                .and_then(|v| v.as_str())
                .unwrap_or("hook");
            let tool_info = body
                .get("tool_name")
                .and_then(|v| v.as_str())
                .map(|t| format!(" [{t}]"))
                .unwrap_or_default();
            log_event("hook", &format!(
                "\x1b[33m{}\x1b[0m {hook_name}{tool_info} \x1b[34m→bus\x1b[0m \x1b[32m+{count}\x1b[0m ({})",
                short_id(&session_id), summary
            ));
        }

        (
            StatusCode::ACCEPTED,
            Json(json!({"status": "ok", "events": count})),
        )
    } else {
        // Local mode (NoopBus): ingest directly
        let result = ingest_events(&mut s, &session_id, &events, project_id.as_deref());
        // Broadcast changes to local WS clients
        for change in &result.changes {
            let _ = s.broadcast_tx.send(change.clone());
        }

        if result.count > 0 {
            let hook_name = body
                .get("hook_event_name")
                .and_then(|v| v.as_str())
                .unwrap_or("hook");
            let tool_info = body
                .get("tool_name")
                .and_then(|v| v.as_str())
                .map(|t| format!(" [{t}]"))
                .unwrap_or_default();
            log_event("hook", &format!(
                "\x1b[33m{}\x1b[0m {hook_name}{tool_info} \x1b[32m+{}\x1b[0m ({})",
                short_id(&session_id), result.count, summary
            ));
        }

        (
            StatusCode::ACCEPTED,
            Json(json!({"status": "ok", "events": result.count})),
        )
    }
}
