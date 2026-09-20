//! `/api/memory` — where a host's judgment lands (memory hands, D-02).
//!
//! The write seam for memory: like `/api/control` for `ui.*`, this is the
//! one door through which anything reaches the `memory` table and the
//! `memory.>` stream. The MCP write hands POST here; `HttpEventStore`
//! stays read-only by construction. Nothing here touches `events.*`.
//!
//! A record is validated before it is stored: the payload must parse as
//! its kind (Enrichment, Reading, Verdict; Saga and Keep are objects with
//! a handle for now), it must carry an author, and a reading must be a
//! lawful grouping of the arc's exchange handles (`validate_reading`).
//! Then: `insert_memory` (SQLite or Mongo), best-effort publish of the
//! stored record on `memory.{kind}.{session}`, and the record is returned.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use open_story_patterns::story::{
    validate_reading, Author, Enrichment, Keep, MemoryKind, MemoryRecord, Reading, Saga, Standing,
    Verdict,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub struct MemoryWrite {
    pub kind: MemoryKind,
    pub handle: String,
    pub session_id: String,
    /// Who read. May also live inside `payload.author`; top-level wins.
    #[serde(default)]
    pub author: Option<Author>,
    pub payload: Value,
}

type ApiError = (StatusCode, Json<Value>);

fn bad(msg: impl Into<String>) -> ApiError {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "ok": false, "error": msg.into() })),
    )
}

fn internal(msg: impl Into<String>) -> ApiError {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({ "ok": false, "error": msg.into() })),
    )
}

/// Pure: check the write against its kind and return the standing it carries.
fn validate(
    write: &MemoryWrite,
    author: &Author,
    arc_exchanges: Option<&[String]>,
) -> Result<Option<Standing>, String> {
    // The payload must agree with the envelope on handle and author.
    let mut payload = write.payload.clone();
    if payload.get("author").is_none() {
        payload["author"] = serde_json::to_value(author).unwrap_or(Value::Null);
    }
    if payload.get("handle").is_none() {
        payload["handle"] = json!(write.handle);
    }
    match write.kind {
        MemoryKind::Enrichment => {
            let e: Enrichment =
                serde_json::from_value(payload).map_err(|e| format!("enrichment payload: {e}"))?;
            if e.handle != write.handle {
                return Err("enrichment.handle must equal handle".into());
            }
            Ok(None)
        }
        MemoryKind::Reading => {
            let r: Reading =
                serde_json::from_value(payload).map_err(|e| format!("reading payload: {e}"))?;
            if r.handle != write.handle {
                return Err("reading.handle must equal handle".into());
            }
            let Some(exchanges) = arc_exchanges else {
                return Err(format!(
                    "no arc {} in session {}",
                    write.handle, write.session_id
                ));
            };
            validate_reading(&r, exchanges)
                .map_err(|e| format!("reading violates the laws: {e}"))?;
            Ok(Some(r.standing))
        }
        MemoryKind::Verdict => {
            let v: Verdict =
                serde_json::from_value(payload).map_err(|e| format!("verdict payload: {e}"))?;
            if v.handle != write.handle {
                return Err("verdict.handle must equal handle".into());
            }
            Ok(None)
        }
        MemoryKind::Saga => {
            let s: Saga =
                serde_json::from_value(payload).map_err(|e| format!("saga payload: {e}"))?;
            if s.handles.len() < 2 {
                return Err("a saga links at least two arcs".into());
            }
            if !s.handles.iter().any(|h| h == &write.handle) {
                return Err("saga.handles must include handle".into());
            }
            Ok(None)
        }
        MemoryKind::Keep => {
            let k: Keep =
                serde_json::from_value(payload).map_err(|e| format!("keep payload: {e}"))?;
            if k.reason.trim().is_empty() {
                return Err("keep.reason must be non-empty".into());
            }
            Ok(None)
        }
    }
}

/// `POST /api/memory { kind, handle, session_id, author, payload }`
pub async fn post_memory(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, ApiError> {
    let write: MemoryWrite =
        serde_json::from_value(body).map_err(|e| bad(format!("invalid memory write: {e}")))?;
    let author = write
        .author
        .clone()
        .or_else(|| {
            write
                .payload
                .get("author")
                .and_then(|a| serde_json::from_value(a.clone()).ok())
        })
        .ok_or_else(|| bad("author { host, model } is required"))?;
    if author.host.trim().is_empty() || author.model.trim().is_empty() {
        return Err(bad("author.host and author.model must be non-empty"));
    }

    let (store, bus) = {
        let s = state.read().await;
        (s.store.event_store.clone(), s.bus.clone())
    };

    // A reading is checked against the arc it groups.
    let arc_exchanges: Option<Vec<String>> = if write.kind == MemoryKind::Reading {
        let arcs = store
            .session_patterns(&write.session_id, Some("story.arc"))
            .await
            .map_err(|e| internal(format!("session_patterns: {e}")))?;
        arcs.iter()
            .find(|a| {
                a.metadata.get("handle").and_then(|h| h.as_str()) == Some(write.handle.as_str())
            })
            .and_then(|a| a.metadata.get("exchanges"))
            .and_then(|v| v.as_array())
            .map(|xs| {
                xs.iter()
                    .filter_map(|h| h.as_str().map(String::from))
                    .collect()
            })
    } else {
        None
    };
    let standing = validate(&write, &author, arc_exchanges.as_deref()).map_err(bad)?;

    let created_at = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let mut payload = write.payload.clone();
    if payload.get("author").is_none() {
        payload["author"] = serde_json::to_value(&author).unwrap_or(Value::Null);
    }
    let record = MemoryRecord::new(
        &write.session_id,
        &write.handle,
        write.kind,
        standing,
        author,
        &created_at,
        payload,
    );

    store
        .insert_memory(&record)
        .await
        .map_err(|e| internal(format!("insert_memory: {e}")))?;

    // Tell open dashboards (D-03). Best-effort: no subscriber is fine.
    {
        let s = state.read().await;
        let _ = s
            .broadcast_tx
            .send(crate::broadcast::BroadcastMessage::Memory {
                record: record.clone(),
            });
    }

    // Best-effort publish of the stored record: durable copy is already in the store.
    let subject = format!("memory.{}.{}", record.kind.as_str(), record.session_id);
    match serde_json::to_vec(&record) {
        Ok(bytes) => {
            if let Err(e) = bus.publish_bytes(&subject, &bytes).await {
                eprintln!("memory publish_bytes({subject}) failed: {e}");
            }
        }
        Err(e) => eprintln!("memory record → json failed: {e}"),
    }

    Ok(Json(serde_json::to_value(&record).unwrap_or(Value::Null)))
}

/// `GET /api/sessions/{session_id}/memory`
pub async fn get_session_memory(
    State(state): State<SharedState>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let store = state.read().await.store.event_store.clone();
    let rows = store
        .session_memory(&session_id)
        .await
        .map_err(|e| internal(format!("session_memory: {e}")))?;
    Ok(Json(json!({ "memory": rows })))
}

/// `GET /api/memory/{handle}`
pub async fn get_memory_for_handle(
    State(state): State<SharedState>,
    Path(handle): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let store = state.read().await.store.event_store.clone();
    let rows = store
        .memory_for_handle(&handle)
        .await
        .map_err(|e| internal(format!("memory_for_handle: {e}")))?;
    Ok(Json(json!({ "memory": rows })))
}

/// `GET /api/story/search?q=&limit=` and `GET /api/memory/search?q=&limit=`
/// (B-11): store-wide text search, so recall reaches every session.
#[derive(Deserialize)]
pub struct SearchParams {
    #[serde(default)]
    pub q: String,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
}

fn default_search_limit() -> usize {
    50
}

pub async fn search_story(
    State(state): State<SharedState>,
    axum::extract::Query(p): axum::extract::Query<SearchParams>,
) -> Result<Json<Value>, ApiError> {
    let q = p.q.trim().to_lowercase();
    if q.is_empty() {
        return Ok(Json(json!({ "patterns": [] })));
    }
    let store = state.read().await.store.event_store.clone();
    let rows = store
        .search_story(&q, p.limit.clamp(1, 500))
        .await
        .map_err(|e| internal(format!("search_story: {e}")))?;
    Ok(Json(json!({ "patterns": rows })))
}

pub async fn search_memory(
    State(state): State<SharedState>,
    axum::extract::Query(p): axum::extract::Query<SearchParams>,
) -> Result<Json<Value>, ApiError> {
    let q = p.q.trim().to_lowercase();
    if q.is_empty() {
        return Ok(Json(json!({ "memory": [] })));
    }
    let store = state.read().await.store.event_store.clone();
    let rows = store
        .search_memory(&q, p.limit.clamp(1, 500))
        .await
        .map_err(|e| internal(format!("search_memory: {e}")))?;
    Ok(Json(json!({ "memory": rows })))
}
