//! Memory hands — read side over the story layer (requirements group B).
//!
//! These tools read `story.arc` / `story.exchange` patterns (folded by
//! `open_story_patterns::story::StoryDetector`) plus `turn.sentence`
//! patterns and raw events, all through the same read-only `EventStore`
//! the other query tools use. No new store handle, no writes.
//!
//! Handles are content addresses (16 hex). A handle names a node; nodes
//! are arcs, exchanges, sentences (handle derived from event ids), and
//! events (their own id). Prefixes of four or more characters resolve.

use open_story_patterns::story::handle as content_handle;
use open_story_patterns::PatternEvent;
use open_story_store::event_store::EventStore;
use serde_json::{json, Value};
use std::sync::Arc;

/// How many sessions a session-less call scans, newest first.
const SESSION_SCAN_LIMIT: usize = 50;

pub fn story_list_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "session_id": {"type": "string", "description": "Restrict to one session (UUID)"},
            "limit": {"type": "integer", "minimum": 1, "description": "Max arcs returned (default 50)"}
        },
        "additionalProperties": false
    })
}

pub fn story_summary_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "handle": {"type": "string", "description": "Arc handle (16 hex) or a prefix of 4+ chars"},
            "session_id": {"type": "string", "description": "Session to look in (recommended; otherwise recent sessions are scanned)"}
        },
        "required": ["handle"],
        "additionalProperties": false
    })
}

fn meta_str<'a>(p: &'a PatternEvent, key: &str) -> Option<&'a str> {
    p.metadata.get(key).and_then(|v| v.as_str())
}

/// One line per arc: the handle a host carries in working memory.
fn arc_line(session_id: &str, arc: &PatternEvent) -> Value {
    let exchanges = arc
        .metadata
        .get("exchanges")
        .and_then(|v| v.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    json!({
        "handle": meta_str(arc, "handle"),
        "session_id": session_id,
        "arc_index": arc.metadata.get("arc_index"),
        "started_at": arc.started_at,
        "ended_at": arc.ended_at,
        "question": meta_str(arc, "question").map(|q| truncate(q, 120)),
        "exchanges": exchanges,
        "closed_by": meta_str(arc, "closed_by"),
        "ambiguous_seams": arc.metadata.get("ambiguous_seams").and_then(|v| v.as_array()).map(|a| a.len()).unwrap_or(0),
    })
}

fn truncate(s: &str, max: usize) -> String {
    open_story_core::strings::truncate_at_char_boundary(s, max).to_string()
}

/// Sessions to scan for a call without `session_id`: newest first, capped.
async fn candidate_sessions(
    store: &Arc<dyn EventStore>,
    session_id: Option<&str>,
) -> Result<Vec<String>, String> {
    if let Some(sid) = session_id {
        return Ok(vec![sid.to_string()]);
    }
    let mut rows = store
        .list_sessions()
        .await
        .map_err(|e| format!("list_sessions failed: {e}"))?;
    rows.sort_by(|a, b| b.last_event.cmp(&a.last_event));
    Ok(rows
        .into_iter()
        .take(SESSION_SCAN_LIMIT)
        .map(|r| r.id)
        .collect())
}

/// `story_list { session_id?, limit? }` → `[arc line]`
pub async fn story_list(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let session_id = args.get("session_id").and_then(|v| v.as_str());
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
    let mut lines = Vec::new();
    for sid in candidate_sessions(store, session_id).await? {
        let arcs = store
            .session_patterns(&sid, Some("story.arc"))
            .await
            .map_err(|e| format!("session_patterns failed: {e}"))?;
        for arc in &arcs {
            lines.push(arc_line(&sid, arc));
            if lines.len() >= limit {
                return Ok(Value::Array(lines));
            }
        }
    }
    Ok(Value::Array(lines))
}

/// A resolved node: which session it lives in and the pattern (or event) itself.
pub struct Found {
    pub session_id: String,
    pub pattern: PatternEvent,
}

/// Find arcs whose handle starts with `prefix` in the candidate sessions.
async fn find_arcs(
    store: &Arc<dyn EventStore>,
    prefix: &str,
    session_id: Option<&str>,
) -> Result<Vec<Found>, String> {
    if prefix.len() < 4 {
        return Err(format!(
            "handle prefix `{prefix}` is too short; give at least 4 characters"
        ));
    }
    let mut hits = Vec::new();
    for sid in candidate_sessions(store, session_id).await? {
        let arcs = store
            .session_patterns(&sid, Some("story.arc"))
            .await
            .map_err(|e| format!("session_patterns failed: {e}"))?;
        for arc in arcs {
            if meta_str(&arc, "handle")
                .map(|h| h.starts_with(prefix))
                .unwrap_or(false)
            {
                hits.push(Found {
                    session_id: sid.clone(),
                    pattern: arc,
                });
            }
        }
    }
    Ok(hits)
}

/// Exactly one arc for a prefix, or an error naming the candidates.
async fn resolve_arc(
    store: &Arc<dyn EventStore>,
    prefix: &str,
    session_id: Option<&str>,
) -> Result<Found, String> {
    let mut hits = find_arcs(store, prefix, session_id).await?;
    match hits.len() {
        0 => Err(format!("no arc matches handle `{prefix}`")),
        1 => Ok(hits.remove(0)),
        _ => {
            let candidates: Vec<String> = hits
                .iter()
                .map(|f| {
                    format!(
                        "{} ({})",
                        meta_str(&f.pattern, "handle").unwrap_or(""),
                        truncate(meta_str(&f.pattern, "question").unwrap_or(""), 60)
                    )
                })
                .collect();
            Err(format!(
                "handle `{prefix}` is ambiguous; candidates: {}",
                candidates.join("; ")
            ))
        }
    }
}

/// `story_summary { handle, session_id? }` → the top-node payload.
///
/// Skeleton fields only. `title` and `slots` appear once an enrichment
/// exists for the handle (group D); today they are absent, not null.
pub async fn story_summary(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let prefix = args
        .get("handle")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "story_summary requires `handle`".to_string())?;
    let session_id = args.get("session_id").and_then(|v| v.as_str());
    let found = resolve_arc(store, prefix, session_id).await?;
    let arc = &found.pattern;
    let handle_s = meta_str(arc, "handle").unwrap_or("");

    // Across: sibling arcs in the same session sharing an entity.
    let mine: std::collections::BTreeSet<String> = arc
        .metadata
        .get("entities")
        .and_then(|v| v.as_object())
        .map(|m| m.keys().cloned().collect())
        .unwrap_or_default();
    let siblings = store
        .session_patterns(&found.session_id, Some("story.arc"))
        .await
        .map_err(|e| format!("session_patterns failed: {e}"))?;
    let across: Vec<Value> = siblings
        .iter()
        .filter(|s| meta_str(s, "handle") != Some(handle_s))
        .filter(|s| {
            s.metadata
                .get("entities")
                .and_then(|v| v.as_object())
                .map(|m| m.keys().any(|k| mine.contains(k)))
                .unwrap_or(false)
        })
        .map(|s| json!(meta_str(s, "handle")))
        .collect();

    Ok(json!({
        "handle": handle_s,
        "session_id": found.session_id,
        "arc_index": arc.metadata.get("arc_index"),
        "started_at": arc.started_at,
        "ended_at": arc.ended_at,
        "question": arc.metadata.get("question"),
        "resolution": arc.metadata.get("resolution"),
        "entities": arc.metadata.get("entities"),
        "tools": arc.metadata.get("tools"),
        "closed_by": arc.metadata.get("closed_by"),
        "ambiguous_seams": arc.metadata.get("ambiguous_seams"),
        "down": arc.metadata.get("exchanges"),
        "across": across,
    }))
}

/// Content address for a sentence: derived from its event ids, the same
/// function arcs and exchanges use, so sentences can be addressed too.
pub fn sentence_handle(p: &PatternEvent) -> String {
    content_handle(&p.event_ids)
}
