//! Search tools: `search` (raw FTS) + `agent_search` (FTS results
//! grouped by session, agent-friendly shape).

use open_story_store::event_store::EventStore;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

pub fn search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {"type": "string", "description": "FTS5 search terms"},
            "limit": {"type": "integer", "minimum": 1, "maximum": 200, "default": 10},
            "session_id": {"type": "string", "description": "Optional — scope to one session"},
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

pub async fn search(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "search requires `query` (string)".to_string())?;
    if query.trim().is_empty() {
        return Err("query must not be empty".to_string());
    }
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
    let session_filter = args.get("session_id").and_then(|v| v.as_str());

    let hits = store
        .search_fts(query, limit, session_filter)
        .await
        .map_err(|e| format!("search_fts failed: {e}"))?;
    serde_json::to_value(hits).map_err(|e| format!("serialize: {e}"))
}

// ── agent_search ────────────────────────────────────────────────

pub fn agent_search_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {"type": "string", "description": "FTS5 search terms"},
            "project": {"type": "string", "description": "Optional project_id/name filter (case-insensitive substring)"},
            "limit": {"type": "integer", "minimum": 1, "maximum": 50, "default": 5,
                      "description": "Max session-level groups to return"},
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

pub async fn agent_search(store: &Arc<dyn EventStore>, args: Value) -> Result<Value, String> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "agent_search requires `query` (string)".to_string())?;
    if query.trim().is_empty() {
        return Err("query must not be empty".to_string());
    }
    let project_filter = args
        .get("project")
        .and_then(|v| v.as_str())
        .map(str::to_lowercase);
    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

    // Search with a higher event limit, then group by session.
    let event_limit = limit * 10;
    let hits = store
        .search_fts(query, event_limit, None)
        .await
        .map_err(|e| format!("search_fts failed: {e}"))?;

    // Build (session_id → project_id) lookup from the session list
    // so we can apply the project filter without needing the live
    // server's session_projects cache.
    let session_project_map: HashMap<String, String> = if project_filter.is_some() {
        store
            .list_sessions()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter_map(|row| row.project_id.map(|p| (row.id, p)))
            .collect()
    } else {
        HashMap::new()
    };

    // Group by session_id.
    let mut by_session: HashMap<String, Vec<&open_story_store::queries::FtsSearchResult>> =
        HashMap::new();
    for hit in &hits {
        by_session
            .entry(hit.session_id.clone())
            .or_default()
            .push(hit);
    }

    let mut groups: Vec<Value> = by_session
        .into_iter()
        .filter_map(|(sid, hits)| {
            if let Some(ref p) = project_filter {
                let session_proj = session_project_map.get(&sid)?;
                if !session_proj.to_lowercase().contains(p) {
                    return None;
                }
            }
            let best_rank = hits.iter().map(|h| h.rank).fold(0.0f64, f64::min);
            let matching: Vec<Value> = hits
                .iter()
                .take(3)
                .map(|h| {
                    json!({
                        "event_id": h.event_id,
                        "rank": h.rank,
                        "snippet": h.snippet,
                        "record_type": h.record_type,
                    })
                })
                .collect();
            Some(json!({
                "session_id": sid,
                "best_rank": best_rank,
                "matching_events": matching,
                "match_count": hits.len(),
            }))
        })
        .collect();

    // Sort by best_rank ascending (FTS5 rank: more-negative = more-relevant).
    groups.sort_by(|a, b| {
        a["best_rank"]
            .as_f64()
            .unwrap_or(0.0)
            .partial_cmp(&b["best_rank"].as_f64().unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    groups.truncate(limit);

    Ok(json!({ "query": query, "results": groups }))
}
