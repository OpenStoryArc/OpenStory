//! The tier-1 ops hands on the node (REQUIREMENTS M-06, M-07): reproject,
//! verify, catch_up, prune. Each changes only what is derived, records an
//! `ops.command.<hand>` event with its result, answers the same key with
//! the same result without acting again, and refuses while the node is
//! not serving. Tier 2 (restarts, resizes) is not here and never will be.

use std::collections::HashMap;
use std::sync::Mutex;

use axum::http::StatusCode;
use open_story_bus::IngestBatch;
use open_story_core::ops::{self, HANDS};
use serde_json::{json, Value};

use crate::state::SharedState;

/// What each key produced, so a retry answers without acting.
static DONE: Mutex<Option<HashMap<String, Value>>> = Mutex::new(None);

fn recorded(key: &str) -> Option<Value> {
    DONE.lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|m| m.get(key).cloned())
}

fn record(key: &str, body: Value) {
    DONE.lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(key.to_string(), body);
}

fn error(status: StatusCode, msg: String) -> (StatusCode, Value) {
    (status, json!({ "error": msg }))
}

/// The refusal while the node is not serving (M-07), naming the progress.
pub fn not_serving_error(boot: &Value) -> Option<String> {
    let phase = boot["phase"].as_str().unwrap_or("unknown");
    if phase == "serving" {
        return None;
    }
    let r = &boot["replay"];
    Some(match (r["done"].as_u64(), r["total"].as_u64()) {
        (Some(d), Some(t)) if t > 0 => {
            format!("node is {phase} ({d} of {t} sessions); tier-1 hands wait for serving")
        }
        _ => format!("node is {phase}; tier-1 hands wait for serving"),
    })
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str().map(str::to_string))
        .collect()
}

/// Run one hand. The body carries the hand's arguments plus
/// `idempotency_key` (required), `author`, and `evidence`.
pub async fn run_hand(state: &SharedState, hand: &str, body: Value) -> (StatusCode, Value) {
    let boot = serde_json::to_value(crate::boot::snapshot()).unwrap_or(Value::Null);
    if let Some(msg) = not_serving_error(&boot) {
        return error(StatusCode::SERVICE_UNAVAILABLE, msg);
    }
    if !HANDS.contains(&hand) {
        return error(
            StatusCode::NOT_FOUND,
            format!(
                "unknown hand `{hand}`; tier-1 hands are {}",
                HANDS.join(", ")
            ),
        );
    }
    let Some(key) = body["idempotency_key"].as_str().filter(|k| !k.is_empty()) else {
        return error(
            StatusCode::BAD_REQUEST,
            "idempotency_key is required".to_string(),
        );
    };
    if let Some(mut done) = recorded(key) {
        done["replayed"] = json!(true);
        return (StatusCode::OK, done);
    }
    let author = body["author"].as_str().unwrap_or("unknown").to_string();
    let evidence = strings(&body["evidence"]);

    let outcome: Result<Value, (StatusCode, String)> = match hand {
        "reproject" => Ok(reproject(state, body["session_id"].as_str()).await),
        "verify" => match body["session_id"].as_str().filter(|s| !s.is_empty()) {
            Some(sid) => Ok(verify(state, sid).await),
            None => Err((
                StatusCode::BAD_REQUEST,
                "session_id is required".to_string(),
            )),
        },
        "catch_up" => {
            let peer = body["peer"]
                .as_str()
                .map(str::to_string)
                .or_else(|| std::env::var("OPEN_STORY_CATCH_UP_PEER").ok())
                .filter(|p| !p.trim().is_empty());
            match peer {
                Some(peer) => Ok(catch_up(state, &peer).await),
                None => Err((
                    StatusCode::BAD_REQUEST,
                    "no peer: pass `peer` or set OPEN_STORY_CATCH_UP_PEER".to_string(),
                )),
            }
        }
        "prune" => match body["older_than_days"].as_u64() {
            Some(days) if days >= 1 => Ok(prune(state, days as u32).await),
            _ => Err((
                StatusCode::BAD_REQUEST,
                "older_than_days must be at least 1".to_string(),
            )),
        },
        _ => unreachable!("hand checked above"),
    };
    let result = match outcome {
        Ok(r) => r,
        Err((status, msg)) => return error(status, msg),
    };
    let ok = result.get("error").is_none();

    // Record the command on the bus (never events.*). A failure to record
    // is logged and reported; the act itself has happened.
    let command_subject = ops::command_subject(hand);
    let ce = ops::command_event(hand, &author, &evidence, key, ok, result.clone());
    let batch = IngestBatch {
        session_id: ops::ops_session_id(hand),
        project_id: ops::SOURCE.to_string(),
        events: vec![ce],
    };
    let bus = state.read().await.bus.clone();
    let recorded_on_bus = match bus.publish(&command_subject, &batch).await {
        Ok(()) => true,
        Err(e) => {
            crate::logging::publish_failed("ops", &command_subject, &batch.session_id, 1, &e);
            false
        }
    };
    tracing::info!(
        event = "ops_command",
        hand,
        author,
        idempotency_key = key,
        ok,
        "ops {hand} by {author}: ok={ok}"
    );

    let response = json!({
        "hand": hand,
        "idempotency_key": key,
        "author": author,
        "evidence": evidence,
        "replayed": false,
        "ok": ok,
        "result": result,
        "command_subject": command_subject,
        "recorded": recorded_on_bus,
    });
    record(key, response.clone());
    (StatusCode::OK, response)
}

/// Rebuild one session's projection from the store, or every session's.
async fn reproject(state: &SharedState, session_id: Option<&str>) -> Value {
    let s = state.read().await;
    match session_id.filter(|id| !id.is_empty()) {
        Some(id) => {
            match open_story_store::rebuild::rebuild_session(s.store.event_store.as_ref(), id).await
            {
                Some(proj) => {
                    let events = proj.event_count();
                    s.store.projections.insert(id.to_string(), proj);
                    json!({ "session_id": id, "sessions_reprojected": 1, "events_applied": events })
                }
                None => json!({ "session_id": id, "sessions_reprojected": 0, "events_applied": 0 }),
            }
        }
        None => {
            let report = crate::reproject::reproject_all(&s.store).await;
            json!({
                "sessions_reprojected": report.sessions_reprojected,
                "events_applied": report.events_applied,
            })
        }
    }
}

/// Do the store and the JSONL backup agree on a session? FTS is reported.
async fn verify(state: &SharedState, session_id: &str) -> Value {
    let (store, data_dir) = {
        let s = state.read().await;
        (s.store.event_store.clone(), s.store.data_dir.clone())
    };
    let store_events = store
        .session_events(session_id)
        .await
        .map(|v| v.len() as u64)
        .unwrap_or(0);
    let jsonl_lines = open_story_store::persistence::SessionStore::new(&data_dir)
        .map(|j| j.load_session(session_id).len() as u64)
        .unwrap_or(0);
    let fts_documents = store.fts_count_for_session(session_id).await.ok().flatten();
    // FTS holds only records with text, so it is reported, never the judge:
    // agreement is the store and its backup holding the same events.
    let agree = store_events == jsonl_lines;
    let fts_unindexed = fts_documents.map(|f| store_events.saturating_sub(f));
    json!({
        "session_id": session_id,
        "store_events": store_events,
        "jsonl_lines": jsonl_lines,
        "fts_documents": fts_documents,
        "fts_unindexed": fts_unindexed,
        "agree": agree,
    })
}

/// One reconciliation pass against a peer.
async fn catch_up(state: &SharedState, peer: &str) -> Value {
    let (store, bus) = {
        let s = state.read().await;
        (s.store.event_store.clone(), s.bus.clone())
    };
    let client = reqwest::Client::new();
    let healed = crate::catch_up::catch_up_once(&store, &bus, peer, &client).await;
    json!({ "peer": peer, "healed": healed })
}

/// Apply the retention policy now, keeping this host's own sessions.
async fn prune(state: &SharedState, older_than_days: u32) -> Value {
    let store = state.read().await.store.event_store.clone();
    match store
        .cleanup_old_sessions(older_than_days, Some(open_story_core::host::host()))
        .await
    {
        Ok(deleted) => json!({ "older_than_days": older_than_days, "deleted": deleted }),
        Err(e) => {
            json!({ "older_than_days": older_than_days, "deleted": 0, "error": format!("{e:#}") })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serving_is_no_error_and_replaying_names_progress() {
        assert!(not_serving_error(&json!({"phase": "serving"})).is_none());
        let msg =
            not_serving_error(&json!({"phase": "replaying", "replay": {"done": 2, "total": 9}}))
                .unwrap();
        assert!(
            msg.contains("replaying (2 of 9 sessions)") && msg.contains("serving"),
            "{msg}"
        );
        assert!(not_serving_error(&json!({"phase": "starting"}))
            .unwrap()
            .contains("starting"));
    }
}
