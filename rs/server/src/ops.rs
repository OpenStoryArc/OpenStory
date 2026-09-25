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
        "converge" => converge(state, &body).await,
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

// ── Converge (consistency C-05) ─────────────────────────────────────────────

/// Rounds a converge run makes before it stops, unless told otherwise.
pub const CONVERGE_MAX_ROUNDS: u64 = 3;
/// How long a round waits after re-injecting for the consumers to persist
/// (eventual consistency: the bus hands the batch to the persist actor).
pub const CONVERGE_SETTLE_MS: u64 = 250;

/// The peers a converge run attempts, and the ones it can only report.
/// Given peers win; then the configured catch-up peer; then every other
/// node's beat that advertises an `api_url`. A beat without one is known
/// by host but not reachable, so it is listed under `unknown`.
async fn converge_peers(state: &SharedState, body: &Value) -> (Vec<String>, Vec<String>) {
    let given = strings(&body["peers"]);
    if !given.is_empty() {
        return (given, vec![]);
    }
    if let Some(p) = std::env::var("OPEN_STORY_CATCH_UP_PEER")
        .ok()
        .filter(|p| !p.trim().is_empty())
    {
        return (vec![p], vec![]);
    }
    let store = state.read().await.store.event_store.clone();
    let me = open_story_core::host::host();
    let mut peers = Vec::new();
    let mut unknown = Vec::new();
    for row in store.latest_presence().await.unwrap_or_default() {
        if row.host == me {
            continue;
        }
        match row.body["api_url"]
            .as_str()
            .filter(|u| !u.trim().is_empty())
        {
            Some(url) => peers.push(url.trim_end_matches('/').to_string()),
            None => unknown.push(row.host.clone()),
        }
    }
    peers.sort();
    peers.dedup();
    unknown.sort();
    (peers, unknown)
}

/// Rebuild the projection of every session that has none resident.
async fn reproject_stale(state: &SharedState) -> usize {
    let s = state.read().await;
    let rows = s
        .store
        .event_store
        .list_sessions()
        .await
        .unwrap_or_default();
    let mut n = 0;
    for row in rows {
        if s.store.projections.get(&row.id).is_some() {
            continue;
        }
        if let Some(proj) =
            open_story_store::rebuild::rebuild_session(s.store.event_store.as_ref(), &row.id).await
        {
            s.store.projections.insert(row.id.clone(), proj);
            n += 1;
        }
    }
    n
}

/// Do the store and the JSONL backup agree on each of `sessions`?
async fn verify_sessions(state: &SharedState, sessions: &[String]) -> Value {
    let (store, data_dir) = {
        let s = state.read().await;
        (s.store.event_store.clone(), s.store.data_dir.clone())
    };
    let jsonl = open_story_store::persistence::SessionStore::new(&data_dir).ok();
    let mut disagreeing = Vec::new();
    for sid in sessions {
        let in_store = store
            .session_event_ids(sid)
            .await
            .map(|v| v.len())
            .unwrap_or(0);
        let in_jsonl = jsonl
            .as_ref()
            .map(|j| j.load_session(sid).len())
            .unwrap_or(0);
        if in_store != in_jsonl {
            disagreeing.push(
                json!({"session_id": sid, "store_events": in_store, "jsonl_lines": in_jsonl}),
            );
        }
    }
    json!({
        "checked": sessions.len(),
        "agree": disagreeing.is_empty(),
        "disagreeing": disagreeing,
    })
}

/// A peer's root digest, if it serves one.
async fn peer_rollup_digest(client: &reqwest::Client, peer: &str) -> Option<String> {
    let body: Value = client
        .get(format!("{peer}/api/digests?rollup=1"))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    body["rollup"]["digest"].as_str().map(str::to_string)
}

/// Converge: for each round, reproject what is stale, catch up against
/// every reachable peer (a union through the existing catch-up path),
/// prune per retention, settle, verify what moved; stop when verify agrees
/// and the roll-up matches every reachable peer, or when a round changed
/// nothing. Never writes an event body.
async fn converge(state: &SharedState, body: &Value) -> Result<Value, (StatusCode, String)> {
    let (peers, unknown) = converge_peers(state, body).await;
    if peers.is_empty() {
        let known = if unknown.is_empty() {
            String::new()
        } else {
            format!(" (present but unadvertised: {})", unknown.join(", "))
        };
        return Err((
            StatusCode::BAD_REQUEST,
            format!(
                "no peer: pass `peers`, set OPEN_STORY_CATCH_UP_PEER, or let peers set advertise_url{known}"
            ),
        ));
    }
    let max_rounds = body["max_rounds"]
        .as_u64()
        .filter(|n| *n >= 1)
        .unwrap_or(CONVERGE_MAX_ROUNDS);
    let settle_ms = body["settle_ms"].as_u64().unwrap_or(CONVERGE_SETTLE_MS);
    let (store, bus, retention_days) = {
        let s = state.read().await;
        (
            s.store.event_store.clone(),
            s.bus.clone(),
            s.config.retention_days,
        )
    };
    let client = reqwest::Client::new();

    let mut rounds = Vec::new();
    let mut touched: Vec<String> = Vec::new();
    let mut last_reports: HashMap<String, crate::catch_up::CatchUpReport> = HashMap::new();
    let mut last_match: HashMap<String, Option<bool>> = HashMap::new();
    let mut converged = false;
    let mut changed = false;
    for round in 1..=max_rounds {
        let reprojected = reproject_stale(state).await;
        let mut healed = serde_json::Map::new();
        let mut pulled_events = 0usize;
        for peer in &peers {
            let r = crate::catch_up::catch_up_report(&store, &bus, peer, &client).await;
            pulled_events += r.pulled_events;
            for sid in &r.healed_sessions {
                if !touched.contains(sid) {
                    touched.push(sid.clone());
                }
            }
            healed.insert(peer.clone(), json!(r.healed));
            last_reports.insert(peer.clone(), r);
        }
        let deleted = if retention_days > 0 {
            store
                .cleanup_old_sessions(retention_days, Some(open_story_core::host::host()))
                .await
                .unwrap_or(0)
        } else {
            0
        };
        let round_changed = reprojected > 0 || pulled_events > 0 || deleted > 0;
        changed |= round_changed;
        if round_changed && settle_ms > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(settle_ms)).await;
        }
        let verify = verify_sessions(state, &touched).await;

        let mine = crate::fleet::rollup(&crate::catch_up::placed_digests(&store).await).digest;
        let mut all_match = true;
        for peer in &peers {
            let theirs = peer_rollup_digest(&client, peer).await;
            let m = theirs.as_ref().map(|d| *d == mine);
            all_match &= m.unwrap_or(false);
            last_match.insert(peer.clone(), m);
        }
        rounds.push(json!({
            "round": round,
            "reprojected": reprojected,
            "healed": healed,
            "pulled_events": pulled_events,
            "deleted": deleted,
            "verify": verify,
            "rollups_match": all_match,
        }));
        converged = all_match && verify["agree"] == json!(true);
        if converged || !round_changed {
            break;
        }
    }

    let peer_rows: Vec<Value> = peers
        .iter()
        .map(|p| {
            let r = last_reports.get(p).cloned().unwrap_or_default();
            json!({
                "url": p,
                "reachable": r.reachable,
                "missing_here": r.missing_here,
                "missing_there": r.missing_there,
                "diverged": r.diverged,
                "rollup_match": last_match.get(p).copied().flatten(),
            })
        })
        .collect();
    Ok(json!({
        "peers": peer_rows,
        "unknown_peers": unknown,
        "rounds": rounds,
        "max_rounds": max_rounds,
        "converged": converged,
        "changed": changed,
    }))
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
