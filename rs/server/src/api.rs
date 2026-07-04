//! REST API handlers — all /api/* routes.

use std::collections::HashMap;
use std::path::Path;

use axum::Json;
use axum::extract::{Path as AxumPath, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{Value, json};

use chrono::{Timelike, Utc};
use open_story_store::analysis::{activity_summary, session_summary, tool_call_distribution};

use crate::broadcast::BroadcastMessage;
use crate::logging::{log_event, short_id};
use crate::state::SharedState;
use crate::tool_schemas::schemas_to_json;
use crate::transcript::{find_transcript_path, read_transcript};

#[derive(Deserialize)]
pub struct SessionListQuery {
    /// Maximum number of sessions to return (default: all).
    pub limit: Option<usize>,
    /// Number of sessions to skip (default: 0). Applied after sort by last_event DESC.
    pub offset: Option<usize>,
    /// Only include sessions with activity at or after this timestamp (RFC 3339).
    pub since: Option<String>,
    /// Only include sessions whose origin host matches this value exactly.
    /// Pre-migration sessions (host: None) never match a host filter.
    pub host: Option<String>,
    /// Only include sessions whose origin user matches this value exactly.
    /// Pre-migration sessions (user: None) never match a user filter.
    pub user: Option<String>,
    /// Sort mode. `latest` (default), `active`, or `tokens`. Unknown values
    /// fall back to `latest` so the UI never sees a 400 from a typo.
    pub sort: Option<String>,
}

/// Aggregate node health — the detailed read companion to the dumb `/health`
/// liveness probe. One view over store / bus / projection state, so the failure
/// modes the architecture review surfaced (disconnected bus, stale projections
/// after restart, etc.) become observable instead of silent. Pure observation.
/// See `docs/research/node-and-network-health.md`. Detailed watcher state lives
/// at `/api/watchers`.
/// `POST /api/control` — the agent-in-UI WRITE seam. Accepts a view intent
/// (`{ action, params?, issuer? }`) and broadcasts it to connected dashboards
/// over the existing WebSocket as a `control` message. The UI (a sink) reacts.
///
/// Sovereignty: this only steers what the dashboard *shows* — it can't touch the
/// observed sources. "Drive the mirror, never the watched." Returns how many
/// dashboards received it (`delivered`).
pub async fn post_control(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let action = body
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if action.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "missing 'action'" })),
        );
    }
    let params = body.get("params").cloned().unwrap_or(Value::Null);
    let issuer = body
        .get("issuer")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    log_event("control", &format!("POST /api/control action={action}"));

    let s = state.read().await;
    // Publish the authored control intent onto the AUTHORED `ui.*` namespace
    // (NEVER `events.*` — that's the observed read-only source), so the drive
    // stream is first-class on the bus: subscribable, federatable, replayable —
    // the write half of the seam, symmetric with the interaction (read) half.
    let at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let subject = crate::ui_events::ui_subject("control", &action, issuer.as_deref());
    let raw = json!({ "action": action.clone(), "params": params.clone(), "issuer": issuer.clone(), "at": at });
    let ce = crate::ui_events::ui_cloud_event("control", &action, VIEWING_SESSION, raw);
    let _ = s.bus.publish(&subject, &crate::ui_events::ui_batch(ce)).await;

    let msg = BroadcastMessage::Control {
        action: action.clone(),
        params,
        issuer,
    };
    // send() errs only when there are no subscribers → 0 delivered.
    let delivered = s.broadcast_tx.send(msg).unwrap_or(0);
    (
        StatusCode::OK,
        Json(json!({ "ok": true, "action": action, "delivered": delivered })),
    )
}

/// `GET /api/viz-candidates` — serve the falsifiable viz design-space catalog
/// (docs/research/viz-candidates.json) so the Lab tab renders it. Read at
/// runtime (cwd-relative to the project root) so the dataset stays the single
/// source of truth. Missing/unparseable → empty list, never an error.
pub async fn get_viz_candidates() -> impl IntoResponse {
    let candidates = std::fs::read_to_string("docs/research/viz-candidates.json")
        .ok()
        .and_then(|s| serde_json::from_str::<Value>(&s).ok())
        .unwrap_or_else(|| json!([]));
    Json(json!({ "candidates": candidates }))
}

/// The synthetic session that holds the human's own interaction stream — their
/// dashboard use, observed as first-class events (the read half of the seam).
const VIEWING_SESSION: &str = "openstory-ui";

/// The interaction kinds we record as distinct subtypes, for high-fidelity
/// replay later. `view` is the coarse fallback.
const INTERACTION_KINDS: [&str; 5] = ["navigate", "filter", "select", "zoom", "view"];

/// `POST /api/interactions` — the UI reports an interaction. We record it as a
/// real CloudEvent in the VIEWING_SESSION so the user's own activity "ends up in
/// OpenStory" (queryable, part of the record, watchable + REPLAYABLE like any
/// session), update the `ui_state` projection, and broadcast it live.
///
/// Body: `{ kind, view, session_id?, filters?, issuer?, …anything }`. `kind`
/// picks the subtype (`interaction.navigate|filter|select|zoom|view`); the WHOLE
/// body is stored as `data` so richer fields (scroll anchor, selected event,
/// canvas zoom/pan) ride along for free as views start sending them — replay
/// fidelity grows without a schema change.
pub async fn post_interaction(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let view = body.get("view").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if view.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(json!({ "ok": false, "error": "view required" })));
    }
    let raw_kind = body.get("kind").and_then(|v| v.as_str()).unwrap_or("view");
    let kind = if INTERACTION_KINDS.contains(&raw_kind) { raw_kind } else { "view" };
    let target = body.get("session_id").and_then(|v| v.as_str()).map(|s| s.to_string());
    let filters = body.get("filters").cloned().filter(|v| !v.is_null());
    let issuer = body.get("issuer").and_then(|v| v.as_str()).map(|s| s.to_string());
    let at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    // The full interaction payload is the authored body (high-fidelity), stamped
    // with the server time — richer fields (scroll anchor, selected event, canvas
    // zoom/pan) ride along for free as views send them. It becomes a proper
    // CloudEvent: the body lives in EventData.raw, subtype interaction.{kind}.
    let mut raw = body.clone();
    if let Some(obj) = raw.as_object_mut() {
        obj.insert("at".to_string(), json!(at));
    }
    let ce = crate::ui_events::ui_cloud_event("interaction", kind, VIEWING_SESSION, raw);
    let event = serde_json::to_value(&ce).unwrap_or(Value::Null);

    let s = state.read().await;
    let _ = s.store.event_store.insert_event(VIEWING_SESSION, &event).await;
    // Publish onto the bus in the AUTHORED `ui.*` namespace (NEVER `events.*` —
    // that's the observed, read-only source) as a TYPED IngestBatch, so the
    // interaction stream is a first-class event source: the MCP subscribes
    // through the same typed pump as observed events, and it's replayable like
    // any other event. Best-effort — never blocks the response.
    let subject = crate::ui_events::ui_subject("interaction", kind, issuer.as_deref());
    let _ = s.bus.publish(&subject, &crate::ui_events::ui_batch(ce)).await;
    let _ = s.broadcast_tx.send(BroadcastMessage::UiState {
        interaction: kind.to_string(),
        view,
        session_id: target,
        filters,
        at,
        issuer,
    });
    (StatusCode::OK, Json(json!({ "ok": true, "kind": kind })))
}

/// `GET /api/ui-state` — the current view state, projected from the latest
/// interaction event. This is what an agent reads to know "where the user is."
pub async fn get_ui_state(State(state): State<SharedState>) -> Json<Value> {
    let s = state.read().await;
    let events = s.store.event_store.session_events(VIEWING_SESSION).await.unwrap_or_default();
    // Unwrap the authored body from the CloudEvent's EventData.raw (tolerant of
    // legacy flat events too), so `where_is_user` sees {view, kind, at, …}.
    let latest = events
        .last()
        .and_then(|e| e.get("data"))
        .map(crate::ui_events::ui_body)
        .unwrap_or(Value::Null);
    // Attention-aware pacing: the rhythm of the human's recent interactions, so
    // an agent can act in their RESTS (drive only when `tempo.active_now` false).
    let times: Vec<i64> = events
        .iter()
        .filter_map(|e| e.get("time").and_then(|v| v.as_str()))
        .filter_map(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        .map(|d| d.timestamp_millis())
        .collect();
    let tempo = crate::ui_events::tempo_profile(times, Utc::now().timestamp_millis());
    Json(json!({ "ui_state": latest, "tempo": tempo }))
}

#[derive(Deserialize)]
pub struct JourneyQuery {
    /// How many recent interactions to return (newest last). Default 20, cap 500.
    pub n: Option<usize>,
}

/// `GET /api/ui-state/journey?n=N` — the recent slice of the human's interaction
/// stream (their PATH through the dashboard), oldest→newest. This is what the
/// REPLAY driver reads: a captured journey fed back through the control seam
/// retraces it (forward) or rewinds it (backward). Returns the raw interaction
/// `data` payloads — each maps 1:1 to a typed Interaction on the client.
pub async fn get_ui_journey(
    State(state): State<SharedState>,
    Query(q): Query<JourneyQuery>,
) -> Json<Value> {
    let n = q.n.unwrap_or(20).min(500);
    let s = state.read().await;
    let events = s.store.event_store.session_events(VIEWING_SESSION).await.unwrap_or_default();
    // Take the last n events (chronological), preserving order — the journey is
    // meaningful only in sequence.
    let start = events.len().saturating_sub(n);
    let journey: Vec<Value> = events[start..]
        .iter()
        .filter_map(|e| e.get("data").map(crate::ui_events::ui_body))
        .collect();
    Json(json!({ "journey": journey }))
}

/// `POST /api/annotations` — pin a durable overlay note to a session. Persists
/// to `{data_dir}/annotations.jsonl` (overlay namespace, never the event
/// stream) and broadcasts `annotation_added` so it appears on every dashboard
/// live. Body: `{ session_id, body, issuer? }`.
pub async fn post_annotation(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> impl IntoResponse {
    let session_id = body.get("session_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let text = body.get("body").and_then(|v| v.as_str()).unwrap_or("").to_string();
    if session_id.is_empty() || text.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "ok": false, "error": "session_id and body are required" })),
        );
    }
    let issuer = body.get("issuer").and_then(|v| v.as_str()).unwrap_or("anon").to_string();
    let ann = crate::annotations::Annotation {
        id: uuid::Uuid::new_v4().to_string(),
        session_id,
        body: text,
        issuer,
        created_at: Utc::now().to_rfc3339(),
    };
    let s = state.read().await;
    let dir = Path::new(&s.config.data_dir);
    if let Err(e) = crate::annotations::append_annotation(dir, &ann) {
        return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "ok": false, "error": e.to_string() })));
    }
    log_event("annotation", &format!("pinned to {}", short_id(&ann.session_id)));
    // Publish the authored annotation onto the `ui.*` namespace (overlay class,
    // NEVER `events.*`). The annotation is user/agent-authored overlay data, so
    // it's a first-class ui event like control + interaction — subscribable and
    // replayable, and the whole authored surface now flows on one sovereign bus.
    let subject = crate::ui_events::ui_subject("annotation", "add", Some(&ann.issuer));
    let raw = serde_json::to_value(&ann).unwrap_or(Value::Null);
    let ce = crate::ui_events::ui_cloud_event("annotation", "add", VIEWING_SESSION, raw);
    let _ = s.bus.publish(&subject, &crate::ui_events::ui_batch(ce)).await;
    let _ = s.broadcast_tx.send(BroadcastMessage::AnnotationAdded { annotation: ann.clone() });
    (StatusCode::OK, Json(json!({ "ok": true, "annotation": ann })))
}

/// `GET /api/annotations[?session_id=…]` — list overlay annotations.
pub async fn list_annotations(
    State(state): State<SharedState>,
    Query(q): Query<HashMap<String, String>>,
) -> Json<Value> {
    let s = state.read().await;
    let dir = Path::new(&s.config.data_dir);
    let mut anns = crate::annotations::read_annotations(dir);
    if let Some(sid) = q.get("session_id") {
        anns.retain(|a| &a.session_id == sid);
    }
    Json(json!({ "annotations": anns }))
}

/// `DELETE /api/annotations/{id}` — remove a durable overlay note. The overlay
/// is user-owned authored data, so it can be deleted (unlike the append-only
/// observed event stream). Broadcasts `annotation_removed` so every dashboard
/// drops it live.
pub async fn delete_annotation(
    State(state): State<SharedState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let s = state.read().await;
    let dir = Path::new(&s.config.data_dir);
    match crate::annotations::remove_annotation(dir, &id) {
        Ok(true) => {
            log_event("annotation", &format!("removed {}", short_id(&id)));
            // Mirror the removal onto the authored `ui.*` bus (never events.*).
            let subject = crate::ui_events::ui_subject("annotation", "remove", None);
            let ce = crate::ui_events::ui_cloud_event(
                "annotation",
                "remove",
                VIEWING_SESSION,
                json!({ "id": id.clone() }),
            );
            let _ = s.bus.publish(&subject, &crate::ui_events::ui_batch(ce)).await;
            let _ = s.broadcast_tx.send(BroadcastMessage::AnnotationRemoved { id: id.clone() });
            (StatusCode::OK, Json(json!({ "ok": true, "removed": id })))
        }
        Ok(false) => (StatusCode::NOT_FOUND, Json(json!({ "ok": false, "error": "not found" }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "ok": false, "error": e.to_string() }))),
    }
}

pub async fn node_health(State(state): State<SharedState>) -> Json<Value> {
    let s = state.read().await;
    let sessions = s
        .store
        .event_store
        .list_sessions()
        .await
        .map(|v| v.len())
        .unwrap_or(0);
    let projections = s.store.projections.len();

    Json(json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "store": {
            "backend": s.config.data_backend.to_string(),
            "sessions": sessions,
        },
        "bus": { "connected": s.bus.is_active() },
        "projections": {
            "count": projections,
            "sessions": sessions,
            // count covers every session ⇒ the read model is rehydrated.
            // Goes false when a restart leaves projections un-rebuilt for
            // source-less sessions (run `reproject`).
            "fresh": projections >= sessions,
        },
        "watchers": s.watcher_diagnostics.snapshots().len(),
    }))
}

/// Per-session convergence digests — the shared primitive for network health
/// (`/api/fleet`) and the `verify` action. Each entry is `(session_id, count,
/// stable event-id hash)`; a peer fetches this and diffs it against its own
/// (see `fleet::diff_digests`) to learn which sessions are converged, missing,
/// or diverged. Cheap and read-only. See `docs/research/node-and-network-health.md`.
pub async fn session_digests(
    State(state): State<SharedState>,
) -> Result<Json<Value>, StatusCode> {
    let s = state.read().await;
    let sessions = s
        .store
        .event_store
        .list_sessions()
        .await
        .unwrap_or_default();

    let mut digests = Vec::with_capacity(sessions.len());
    for row in &sessions {
        let events = s
            .store
            .event_store
            .session_events(&row.id)
            .await
            .unwrap_or_default();
        let ids: Vec<String> = events
            .iter()
            .filter_map(|e| e.get("id").and_then(|v| v.as_str()).map(String::from))
            .collect();
        digests.push(crate::fleet::SessionDigest {
            count: ids.len(),
            digest: crate::fleet::digest_event_ids(&ids),
            session_id: row.id.clone(),
        });
    }

    Ok(Json(json!({ "sessions": digests })))
}

pub async fn list_sessions(
    State(state): State<SharedState>,
    Query(query): Query<SessionListQuery>,
) -> Json<Value> {
    let s = state.read().await;
    let all_rows = s
        .store
        .event_store
        .list_sessions()
        .await
        .unwrap_or_default();

    // Filter by `since` if provided (compare last_event timestamp strings lexicographically —
    // they're RFC 3339 so lexicographic order == chronological order).
    let since_filtered: Vec<&_> = if let Some(ref since) = query.since {
        all_rows
            .iter()
            .filter(|r| r.last_event.as_deref().unwrap_or("") >= since.as_str())
            .collect()
    } else {
        all_rows.iter().collect()
    };

    // Host filter: exact match. Sessions with host: None never match — this
    // is deliberate. A filter like ?host=Maxs-Air should not leak legacy
    // rows whose origin we simply don't know.
    let host_filtered: Vec<&_> = if let Some(ref host) = query.host {
        since_filtered
            .into_iter()
            .filter(|r| r.host.as_deref() == Some(host.as_str()))
            .collect()
    } else {
        since_filtered
    };

    // User filter: same exact-match semantics as host. Both can be combined:
    // `?host=Katies-Mac-mini&user=katie` narrows to sessions matching both.
    let user_filtered: Vec<&_> = if let Some(ref user) = query.user {
        host_filtered
            .into_iter()
            .filter(|r| r.user.as_deref() == Some(user.as_str()))
            .collect()
    } else {
        host_filtered
    };

    // Sort mode. `latest` (default) is a no-op — EventStore::list_sessions
    // returns rows already sorted by last_event DESC, enforced by the
    // conformance helper `it_lists_sessions_ordered_by_last_event_desc`.
    // `active` sorts by event_count DESC; `tokens` sorts by total tokens
    // (input + output) DESC, looked up from the live projections map.
    // Sort is stable, so ties fall back to the underlying last_event order.
    let mut filtered: Vec<&_> = user_filtered;
    match query.sort.as_deref() {
        Some("active") => {
            filtered.sort_by_key(|r| std::cmp::Reverse(r.event_count));
        }
        Some("tokens") => {
            let token_total = |sid: &str| -> u64 {
                s.store
                    .projections
                    .get(sid)
                    .map(|p| p.total_input_tokens() + p.total_output_tokens())
                    .unwrap_or(0)
            };
            filtered.sort_by_key(|r| std::cmp::Reverse(token_total(&r.id)));
        }
        _ => {} // "latest" or unknown → no-op
    }
    let total = filtered.len();

    // Apply offset/limit after sorting.
    let offset = query.offset.unwrap_or(0);
    let page: Vec<&&_> = match query.limit {
        Some(limit) => filtered.iter().skip(offset).take(limit).collect(),
        None => filtered.iter().skip(offset).collect(),
    };

    log_event(
        "api",
        &format!(
            "GET /api/sessions ({}/{} sessions, offset={}, limit={:?})",
            page.len(),
            total,
            offset,
            query.limit,
        ),
    );

    // Per-session plan counts from the durable plan store (one pass, reused for
    // every row). The sidebar plan badge reads this instead of counting
    // ExitPlanMode events, which the loaded record window can miss for older
    // sessions — keeping the badge in sync with the Plans tab.
    let plan_counts: std::collections::HashMap<String, usize> = {
        let mut m = std::collections::HashMap::new();
        for p in s.store.plan_store.list_plans() {
            *m.entry(p.session_id).or_insert(0) += 1;
        }
        m
    };

    // Build response from SessionRow + projections (no per-session event loading).
    // Detailed fields (tool_calls, files_edited, model, etc.) are available via
    // GET /api/sessions/{id}/summary when a specific session is selected.
    let mut result = Vec::new();
    for row in &page {
        let sid = row.id.as_str();
        let project_id = s.store.session_projects.get(sid).map(|r| r.value().clone());
        let project_name = s
            .store
            .session_project_names
            .get(sid)
            .map(|r| r.value().clone());
        let (label, branch, total_input_tokens, total_output_tokens) =
            match s.store.projections.get(sid) {
                Some(proj) => (
                    proj.label().map(|s| s.to_string()),
                    proj.branch().map(|s| s.to_string()),
                    proj.total_input_tokens(),
                    proj.total_output_tokens(),
                ),
                None => (None, None, 0, 0),
            };
        let status = status_from_last_event(row.last_event.as_deref());
        result.push(json!({
            "session_id": sid,
            "status": status,
            "start_time": row.first_event,
            "last_event": row.last_event,
            "event_count": row.event_count,
            "project_id": project_id.as_ref().or(row.project_id.as_ref()),
            "project_name": project_name.as_ref().or(row.project_name.as_ref()),
            "label": label.or(row.label.clone()),
            "branch": branch,
            "total_input_tokens": total_input_tokens,
            "total_output_tokens": total_output_tokens,
            "plan_count": plan_counts.get(sid).copied().unwrap_or(0),
            "host": row.host,
            "user": row.user,
            "origin_agent": row.origin_agent,
            "person_id": row.person_id,
            "principal_id": row.principal_id,
        }));
    }
    Json(json!({
        "sessions": result,
        "total": total,
    }))
}

pub async fn list_watchers(State(state): State<SharedState>) -> Json<Value> {
    let diagnostics = {
        let s = state.read().await;
        s.watcher_diagnostics.clone()
    };
    Json(json!({
        "watchers": diagnostics.snapshots(),
    }))
}

/// Number of recent sessions returned per user in `/api/users`.
const USERS_RECENT_SESSIONS_PER_USER: usize = 5;

/// `GET /api/local-info` — what `OPEN_STORY_HOST` / `OPEN_STORY_USER`
/// resolved to inside this process.
///
/// Lets the UI distinguish "this is *my* session" from "this is replicated
/// from another machine". Specifically, the Live tab's session header uses
/// it to show a "Replicated from another machine" indicator when a viewed
/// session's `user` differs from the local resolver's value.
///
/// Both fields are always present (the resolver falls back to `"unknown"`
/// rather than returning Option), so the response shape is stable.
pub async fn list_local_info(State(_state): State<SharedState>) -> Json<Value> {
    log_event("api", "GET /api/local-info");
    Json(json!({
        "host": open_story_core::host::host(),
        "user": open_story_core::user::user(),
    }))
}

/// `GET /api/fleet` — return the configured Person + their principals.
///
/// Source of truth for the UI's "your fleet" sidebar: provides the
/// display_name for each `principal_id` referenced by sessions, plus
/// the person who owns them. Returns 404 when no `[person]` section is
/// configured (the bootstrap should have populated this on first boot;
/// 404 here means the bootstrap was skipped, e.g. in tests with a
/// hand-rolled Config::default()).
pub async fn get_fleet(State(state): State<SharedState>) -> impl IntoResponse {
    log_event("api", "GET /api/fleet");
    let s = state.read().await;
    match &s.config.person {
        Some(person) => {
            let body = json!({
                "person": {
                    "id": person.id,
                    "display_name": person.display_name,
                    "email": person.email,
                },
                "principals": person.principals.iter().map(|p| json!({
                    "id": p.id,
                    "display_name": p.display_name,
                    "matchers": {
                        "agent": p.matchers.agent,
                        "host": p.matchers.host,
                        "user": p.matchers.user,
                        "watch_dir_pattern": p.matchers.watch_dir_pattern,
                    },
                })).collect::<Vec<_>>(),
            });
            (StatusCode::OK, Json(body)).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "no [person] section configured" })),
        )
            .into_response(),
    }
}

/// `GET /api/users` — aggregate `SessionRow` rows by the `user` field.
///
/// Returns one entry per distinct stamped user, sorted by `last_active` DESC.
/// Sessions with `user: None` (legacy / pre-PR-#42) are excluded — same
/// posture as the `?user=` filter on `/api/sessions`: a "Users" tab
/// shouldn't invent an "Unknown" bucket from rows whose origin we don't know.
///
/// Each entry includes:
///
///   - aggregate counts (session_count, total tokens),
///   - the set of hosts and projects this user has worked from,
///   - last activity timestamp,
///   - the N most-recent sessions (default 5) — the deterministic
///     stand-in for "what they're doing" until the InsightExtraction
///     consumer ships and the UI swaps in real semantic insights.
pub async fn list_users(State(state): State<SharedState>) -> Json<Value> {
    use std::collections::BTreeMap;

    let s = state.read().await;
    let all_rows = s
        .store
        .event_store
        .list_sessions()
        .await
        .unwrap_or_default();

    /// Mutable per-user aggregate built up in a single pass.
    struct Acc {
        session_count: usize,
        hosts: std::collections::BTreeSet<String>,
        projects: std::collections::BTreeSet<String>,
        last_active: Option<String>,
        total_input_tokens: u64,
        total_output_tokens: u64,
        // 24 hourly buckets covering the last 24h; index 0 = oldest hour
        // (now − 24h), index 23 = current hour. Each session whose
        // [first_event, last_event] span overlaps a bucket contributes
        // its event_count proportionally to the overlap. Approximation —
        // assumes uniform event rate across each session's span.
        activity_24h: [u64; 24],
        // Held as references so we can render `recent_sessions` after sorting
        // by last_event without cloning intermediate state.
        sessions: Vec<usize>, // indexes into `all_rows`
    }

    // BTreeMap so the iteration order is deterministic when last_active
    // comparisons tie (rare, but tests appreciate it).
    let mut by_user: BTreeMap<String, Acc> = BTreeMap::new();

    // Anchor the activity_24h window at the current minute floor of the
    // current hour. Bucket 23 is the in-progress hour; bucket 0 is the
    // hour 23h ago. Computed once outside the loop so all users share
    // the same time origin.
    let window_now = chrono::Utc::now();
    let window_end = window_now
        .date_naive()
        .and_hms_opt(window_now.hour(), 0, 0)
        .map(|d| d.and_utc() + chrono::Duration::hours(1))
        .unwrap_or(window_now);
    let window_start = window_end - chrono::Duration::hours(24);

    for (idx, row) in all_rows.iter().enumerate() {
        let Some(user) = row.user.as_deref() else {
            continue; // legacy / pre-stamping rows
        };
        let acc = by_user.entry(user.to_string()).or_insert_with(|| Acc {
            session_count: 0,
            hosts: Default::default(),
            projects: Default::default(),
            last_active: None,
            total_input_tokens: 0,
            total_output_tokens: 0,
            activity_24h: [0u64; 24],
            sessions: Vec::new(),
        });
        acc.session_count += 1;
        if let Some(host) = row.host.as_deref() {
            acc.hosts.insert(host.to_string());
        }
        // project_name preferred for display; fall back to project_id.
        if let Some(pn) = row.project_name.as_deref().or(row.project_id.as_deref()) {
            acc.projects.insert(pn.to_string());
        }
        // RFC 3339 → lexicographic == chronological, so string max works.
        if let Some(ts) = row.last_event.as_deref() {
            match acc.last_active.as_deref() {
                Some(cur) if cur >= ts => {}
                _ => acc.last_active = Some(ts.to_string()),
            }
        }
        // Live token totals come from the projections map, which the persist
        // consumer keeps fresh; SessionRow's tokens lag by one batch.
        if let Some(proj) = s.store.projections.get(&row.id) {
            acc.total_input_tokens += proj.total_input_tokens();
            acc.total_output_tokens += proj.total_output_tokens();
        }

        // Distribute this session's events across the 24h activity window.
        // We don't store per-event timestamps here, so we approximate:
        // assume events are uniformly distributed over [first_event,
        // last_event], compute the per-hour rate, and add each hour's
        // share to the corresponding bucket. Sessions entirely outside
        // the 24h window contribute nothing. Single-instant sessions
        // (first == last) drop everything in the last_event hour.
        if let (Some(first_str), Some(last_str)) =
            (row.first_event.as_deref(), row.last_event.as_deref())
        {
            if let (Ok(first_dt), Ok(last_dt)) = (
                chrono::DateTime::parse_from_rfc3339(first_str),
                chrono::DateTime::parse_from_rfc3339(last_str),
            ) {
                let first = first_dt.with_timezone(&chrono::Utc);
                let last = last_dt.with_timezone(&chrono::Utc);
                // Clamp to the window. If span is fully before the window,
                // nothing contributes.
                let span_start = first.max(window_start);
                let span_end = last.min(window_end);
                if span_end > span_start {
                    let total_secs = (last - first).num_seconds().max(1) as f64;
                    let rate_per_sec = row.event_count as f64 / total_secs;

                    // Walk hour buckets that overlap the clamped span.
                    let mut bucket_start = span_start;
                    while bucket_start < span_end {
                        let next_hour = (bucket_start + chrono::Duration::hours(1))
                            .date_naive()
                            .and_hms_opt((bucket_start + chrono::Duration::hours(1)).hour(), 0, 0)
                            .map(|d| d.and_utc())
                            .unwrap_or(bucket_start + chrono::Duration::hours(1));
                        let bucket_end = next_hour.min(span_end);
                        let overlap_secs = (bucket_end - bucket_start).num_seconds().max(0) as f64;
                        let bucket_idx =
                            ((bucket_start - window_start).num_hours().clamp(0, 23)) as usize;
                        acc.activity_24h[bucket_idx] +=
                            (overlap_secs * rate_per_sec).round() as u64;
                        bucket_start = bucket_end;
                    }
                }
            }
        }

        acc.sessions.push(idx);
    }

    let mut users: Vec<Value> = by_user
        .into_iter()
        .map(|(user, acc)| {
            // Recent sessions: take the top-N by last_event DESC.
            let mut session_idxs = acc.sessions;
            session_idxs.sort_by_key(|&i| {
                std::cmp::Reverse(all_rows[i].last_event.clone().unwrap_or_default())
            });
            let recent: Vec<Value> = session_idxs
                .into_iter()
                .take(USERS_RECENT_SESSIONS_PER_USER)
                .map(|i| {
                    let r = &all_rows[i];
                    json!({
                        "session_id": r.id,
                        "label": r.label,
                        "last_event": r.last_event,
                        "project_name": r.project_name.as_ref().or(r.project_id.as_ref()),
                        "event_count": r.event_count,
                    })
                })
                .collect();

            json!({
                "user": user,
                "session_count": acc.session_count,
                "hosts": acc.hosts.into_iter().collect::<Vec<_>>(),
                "projects": acc.projects.into_iter().collect::<Vec<_>>(),
                "last_active": acc.last_active,
                "total_input_tokens": acc.total_input_tokens,
                "total_output_tokens": acc.total_output_tokens,
                "activity_24h": acc.activity_24h.to_vec(),
                "recent_sessions": recent,
            })
        })
        .collect();

    // Most-recently-active user first. Users with no last_active sort last.
    users.sort_by_key(|u| {
        std::cmp::Reverse(
            u.get("last_active")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        )
    });

    log_event("api", &format!("GET /api/users ({} users)", users.len()));
    Json(json!({
        "users": users,
        "total": all_rows.len(),
    }))
}

/// `GET /api/sessions/{session_id}/events` — full event stream for a session.
///
/// Unknown sessions return `200` with an empty array.
pub async fn get_events(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let s = state.read().await;
    let events = s
        .store
        .event_store
        .session_events(&session_id)
        .await
        .unwrap_or_default();
    log_event(
        "api",
        &format!(
            "GET /api/sessions/{}/events ({} events)",
            short_id(&session_id),
            events.len()
        ),
    );
    Ok(Json(Value::Array(events)))
}

/// The ONE status rule every surface uses: an event within the last
/// 5 minutes means ongoing, anything else completed. Derived from the
/// store row's last_event so /api/sessions and /summary can't disagree.
fn status_from_last_event(last_event: Option<&str>) -> &'static str {
    match last_event {
        Some(ts) => match chrono::DateTime::parse_from_rfc3339(ts) {
            Ok(t) if Utc::now().signed_duration_since(t).num_seconds() <= 300 => "ongoing",
            _ => "completed",
        },
        None => "completed",
    }
}

pub async fn get_summary(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    log_event(
        "api",
        &format!("GET /api/sessions/{}/summary", short_id(&session_id)),
    );
    let s = state.read().await;

    // Served from the in-memory projection (O(1), maintained incrementally by
    // the projections consumer, rebuilt at boot). Falls back to the whole-
    // session scan only when no projection exists — on a 16.7k-event session
    // that scan costs ~0.9 s per request.
    let (summary, extras) = match s.store.projections.get(&session_id) {
        Some(proj) => {
            let p = proj.value();
            let tokens = json!({
                "input": p.total_input_tokens(),
                "output": p.total_output_tokens(),
                "cache_creation": p.total_cache_creation_tokens(),
                "cache_read": p.total_cache_read_tokens(),
                "total": p.total_input_tokens()
                    + p.total_output_tokens()
                    + p.total_cache_creation_tokens()
                    + p.total_cache_read_tokens(),
            });
            let top_files: Vec<Value> = p
                .top_files(5)
                .into_iter()
                .map(|(path, count)| json!({"path": path, "count": count}))
                .collect();
            (
                p.summary(&session_id, Some(Utc::now())),
                Some((p.turn_count(), tokens, top_files)),
            )
        }
        None => {
            let events = s
                .store
                .event_store
                .session_events(&session_id)
                .await
                .unwrap_or_default();
            (
                session_summary(&session_id, &events, Some(Utc::now())),
                None,
            )
        }
    };

    // Status, event_count, and last_event come from the store's session row —
    // the same source the sessions list reads — so the two surfaces can never
    // disagree. (The projection can drift slightly from the store when boot
    // backfill re-publishes events with fresh ids; the store row is the
    // deduplicated truth.)
    let row = s
        .store
        .event_store
        .list_sessions()
        .await
        .unwrap_or_default()
        .into_iter()
        .find(|r| r.id == session_id);
    let (status, event_count, last_event) = match &row {
        Some(r) => (
            status_from_last_event(r.last_event.as_deref()).to_string(),
            r.event_count as usize,
            r.last_event.clone(),
        ),
        None => (summary.status.clone(), summary.event_count, None),
    };

    let project_id = s
        .store
        .session_projects
        .get(&session_id)
        .map(|r| r.value().clone());
    let mut body = json!({
        "session_id": summary.session_id,
        "status": status,
        "start_time": summary.start_time,
        "last_event": last_event,
        "duration_ms": summary.duration_ms,
        "event_count": event_count,
        "error_count": summary.error_count,
        "tool_calls": summary.tool_calls,
        "files_edited": summary.files_edited,
        "unique_tools": summary.unique_tools,
        "exit_code": summary.exit_code,
        "model": summary.model,
        "prompt_count": summary.prompt_count,
        "response_count": summary.response_count,
        "project_id": project_id,
    });
    if let Some((turn_count, tokens, top_files)) = extras {
        let obj = body.as_object_mut().expect("body is an object");
        obj.insert("turn_count".to_string(), json!(turn_count));
        obj.insert("tokens".to_string(), tokens);
        obj.insert("top_files".to_string(), json!(top_files));
    }
    Json(body)
}

pub async fn get_activity(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    log_event(
        "api",
        &format!("GET /api/sessions/{}/activity", short_id(&session_id)),
    );
    let s = state.read().await;
    let events = s
        .store
        .event_store
        .session_events(&session_id)
        .await
        .unwrap_or_default();
    let a = activity_summary(&events);
    Json(json!({
        "first_prompt": a.first_prompt,
        "files_touched": a.files_touched,
        "tool_breakdown": a.tool_breakdown,
        "error_messages": a.error_messages,
        "last_response": a.last_response,
        "conversation_turns": a.conversation_turns,
        "plan_count": a.plan_count,
        "duration_ms": a.duration_ms,
        "start_time": a.start_time,
    }))
}

pub async fn get_tools(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    let s = state.read().await;
    let events = s
        .store
        .event_store
        .session_events(&session_id)
        .await
        .unwrap_or_default();
    let dist = tool_call_distribution(&events);
    Json(serde_json::to_value(dist).unwrap_or(json!({})))
}

#[derive(Deserialize)]
pub struct TranscriptQuery {
    #[serde(default)]
    pub assistant_only: bool,
}

pub async fn get_transcript(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<TranscriptQuery>,
) -> Json<Value> {
    log_event(
        "api",
        &format!("GET /api/sessions/{}/transcript", short_id(&session_id)),
    );
    let s = state.read().await;
    let events = s
        .store
        .event_store
        .session_events(&session_id)
        .await
        .unwrap_or_default();
    let transcript_path = find_transcript_path(&events);

    let data_dir = s.store.data_dir.clone();
    drop(s); // Release the read lock before doing file I/O

    let transcript_path = match transcript_path {
        Some(p) => {
            let path = Path::new(&p);
            // Reject paths containing traversal components
            let path_str = p.replace('\\', "/");
            if path_str.contains("..") {
                return Json(json!({
                    "error": "invalid transcript path",
                    "entries": []
                }));
            }
            if path.is_absolute() && path.exists() {
                p
            } else {
                // Try resolving relative to data_dir first
                let from_data = data_dir.join(&p);
                if let Ok(canonical) = from_data.canonicalize() {
                    if let Ok(canonical_data) = data_dir.canonicalize() {
                        if canonical.starts_with(&canonical_data) {
                            canonical.to_string_lossy().to_string()
                        } else {
                            return Json(json!({
                                "error": "transcript path outside data directory",
                                "entries": []
                            }));
                        }
                    } else {
                        p
                    }
                } else {
                    p
                }
            }
        }
        None => {
            // Fallback: reconstruct transcript from stored events.
            // Hermes sessions (and any agent that ingests via the plugin/watcher
            // path) don't have a transcript_path — the events ARE the transcript.
            let mut entries: Vec<Value> = Vec::new();
            for ev in &events {
                let raw = ev.get("data").and_then(|d| d.get("raw")).unwrap_or(ev);
                let data = raw.get("data").unwrap_or(raw);
                let role = data.get("role").and_then(|v| v.as_str()).unwrap_or("");
                if role.is_empty() {
                    continue;
                }
                let content = data.get("content").and_then(|v| v.as_str()).unwrap_or("");
                let subtype = ev.get("subtype").and_then(|v| v.as_str()).unwrap_or("");
                let kind = if subtype.contains("tool_use") {
                    "tool_call"
                } else if subtype.contains("tool_result") {
                    "tool_result"
                } else if subtype.contains("thinking") {
                    "thinking"
                } else {
                    "text"
                };
                let mut entry = json!({
                    "role": match role {
                        "tool" => "user",
                        _ => role,
                    },
                    "kind": kind,
                    "content": content,
                });
                // Add tool info if present
                if let Some(ap) = ev.get("data").and_then(|d| d.get("agent_payload")) {
                    if let Some(tool) = ap.get("tool").and_then(|v| v.as_str()) {
                        entry["tool"] = json!(tool);
                    }
                    if let Some(args) = ap.get("args") {
                        entry["args"] = args.clone();
                    }
                }
                if !query.assistant_only || (role == "assistant" && kind == "text") {
                    entries.push(entry);
                }
            }
            return Json(json!({
                "source": "events",
                "entries": entries,
            }));
        }
    };

    let mut entries = read_transcript(Path::new(&transcript_path));
    if query.assistant_only {
        entries.retain(|e| e.role == "assistant" && e.kind == "text");
    }

    Json(json!({
        "path": transcript_path,
        "entries": entries,
    }))
}

// ---------------------------------------------------------------------------
// View-model endpoints (typed records from open-story-views)
// ---------------------------------------------------------------------------

pub async fn get_view_records(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    log_event(
        "api",
        &format!("GET /api/sessions/{}/view-records", short_id(&session_id)),
    );
    let s = state.read().await;
    let events = s
        .store
        .event_store
        .session_events(&session_id)
        .await
        .unwrap_or_default();

    let view_records: Vec<Value> = events
        .iter()
        .filter_map(|event| {
            serde_json::from_value::<open_story_core::cloud_event::CloudEvent>(event.clone()).ok()
        })
        .flat_map(|ce| open_story_views::from_cloud_event::from_cloud_event(&ce))
        .filter_map(|vr| serde_json::to_value(vr).ok())
        .collect();

    Json(Value::Array(view_records))
}

#[derive(Deserialize)]
pub struct ConversationQuery {
    /// Output format: json (default) or markdown
    #[serde(default)]
    pub format: Option<String>,
}

pub async fn get_conversation(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<ConversationQuery>,
) -> axum::response::Response {
    let fmt = query.format.as_deref().unwrap_or("json");
    log_event(
        "api",
        &format!(
            "GET /api/sessions/{}/conversation?format={fmt}",
            short_id(&session_id)
        ),
    );
    let s = state.read().await;
    let events = s
        .store
        .event_store
        .session_events(&session_id)
        .await
        .unwrap_or_default();

    let view_records: Vec<_> = events
        .iter()
        .filter_map(|event| {
            serde_json::from_value::<open_story_core::cloud_event::CloudEvent>(event.clone()).ok()
        })
        .flat_map(|ce| open_story_views::from_cloud_event::from_cloud_event(&ce))
        .collect();

    let paired = open_story_views::pair::pair_records(&view_records);

    match fmt {
        "markdown" | "md" => {
            let md = open_story_views::markdown::conversation_to_markdown(&paired, &session_id);
            axum::response::Response::builder()
                .header("content-type", "text/markdown; charset=utf-8")
                .body(axum::body::Body::from(md))
                .unwrap()
        }
        "html" => {
            let md = open_story_views::markdown::conversation_to_markdown(&paired, &session_id);
            let title = format!("Session {}", &session_id[..12.min(session_id.len())]);
            let html = open_story_views::html::markdown_to_html_page(&md, &title);
            axum::response::Response::builder()
                .header("content-type", "text/html; charset=utf-8")
                .body(axum::body::Body::from(html))
                .unwrap()
        }
        _ => axum::response::Json(serde_json::to_value(paired).unwrap_or(json!({"entries": []})))
            .into_response(),
    }
}

pub async fn get_file_changes(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    log_event(
        "api",
        &format!("GET /api/sessions/{}/file-changes", short_id(&session_id)),
    );
    let s = state.read().await;
    let events = s
        .store
        .event_store
        .session_events(&session_id)
        .await
        .unwrap_or_default();

    let view_records: Vec<_> = events
        .iter()
        .filter_map(|event| {
            serde_json::from_value::<open_story_core::cloud_event::CloudEvent>(event.clone()).ok()
        })
        .flat_map(|ce| open_story_views::from_cloud_event::from_cloud_event(&ce))
        .collect();

    let edits: Vec<Value> = open_story_views::filter::file_edits(&view_records)
        .into_iter()
        .filter_map(|vr| serde_json::to_value(vr).ok())
        .collect();

    Json(Value::Array(edits))
}

pub async fn get_tool_schemas() -> Json<Value> {
    Json(schemas_to_json())
}

/// GET /api/sessions/{session_id}/meta
///
/// Returns cached projection metadata: event_count and filter_counts.
/// O(1) — reads from the projection cache, never iterates rows.
pub async fn get_session_meta(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    log_event(
        "api",
        &format!("GET /api/sessions/{}/meta", short_id(&session_id)),
    );
    let s = state.read().await;
    let proj = s
        .store
        .projections
        .get(&session_id)
        .ok_or(StatusCode::NOT_FOUND)?;
    let meta = proj.query_meta();
    Ok(Json(json!({
        "event_count": meta.event_count,
        "filter_counts": meta.filter_counts,
    })))
}

/// GET /api/sessions/{session_id}/events/{event_id}/content
///
/// Returns the full (untruncated) payload for a truncated record.
/// Returns 404 if the session/event doesn't exist or wasn't truncated.
pub async fn get_event_content(
    State(state): State<SharedState>,
    AxumPath((session_id, event_id)): AxumPath<(String, String)>,
) -> Result<String, StatusCode> {
    log_event(
        "api",
        &format!(
            "GET /api/sessions/{}/events/{}/content",
            short_id(&session_id),
            short_id(&event_id)
        ),
    );
    let s = state.read().await;
    // Try in-memory cache first, then fall back to EventStore.
    // Key is (session_id, event_id) — the DashMap guard derefs to `&String`.
    if let Some(entry) = s
        .store
        .full_payloads
        .get(&(session_id.clone(), event_id.clone()))
    {
        return Ok(entry.value().clone());
    }
    // Fall back: extract tool output from full event payload in EventStore
    let payload = s
        .store
        .event_store
        .full_payload(&event_id)
        .await
        .ok()
        .flatten()
        .ok_or(StatusCode::NOT_FOUND)?;
    let val: Value =
        serde_json::from_str(&payload).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    // Extract tool_result content from the CloudEvent
    let output = val
        .pointer("/data/raw/message/content")
        .and_then(|c| c.as_array())
        .and_then(|blocks| {
            blocks.iter().find_map(|b| {
                if b.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                    b.get("content")
                        .and_then(|c| c.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            })
        })
        .or_else(|| {
            val.pointer("/data/output")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        })
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(output)
}

#[derive(Deserialize)]
pub struct PatternQuery {
    #[serde(rename = "type")]
    pub pattern_type: Option<String>,
}

/// GET /api/sessions/{session_id}/patterns
///
/// Returns all detected patterns for a session. Optional `?type=` query
/// parameter filters by pattern_type (e.g., `?type=git.workflow`).
pub async fn get_patterns(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<PatternQuery>,
) -> Json<Value> {
    log_event(
        "api",
        &format!("GET /api/sessions/{}/patterns", short_id(&session_id)),
    );
    let s = state.read().await;
    let result = s
        .store
        .event_store
        .session_patterns(&session_id, query.pattern_type.as_deref())
        .await
        .unwrap_or_default();
    Json(json!({ "patterns": result }))
}

pub async fn get_turns(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    log_event(
        "api",
        &format!("GET /api/sessions/{}/turns", short_id(&session_id)),
    );
    let s = state.read().await;
    let turns = s
        .store
        .event_store
        .session_turns(&session_id)
        .await
        .unwrap_or_default();
    Json(json!({ "turns": turns }))
}

pub async fn list_plans(State(state): State<SharedState>) -> Json<Value> {
    let s = state.read().await;
    let plans: Vec<Value> = s
        .store
        .plan_store
        .list_plans()
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "session_id": p.session_id,
                "title": p.title,
                "timestamp": p.timestamp,
            })
        })
        .collect();
    Json(Value::Array(plans))
}

pub async fn get_plan(
    State(state): State<SharedState>,
    AxumPath(plan_id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    let s = state.read().await;
    match s.store.plan_store.load(&plan_id) {
        Some(plan) => Ok(Json(serde_json::to_value(plan).unwrap_or(json!({})))),
        None => Err(StatusCode::NOT_FOUND),
    }
}

pub async fn get_session_plans(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    let s = state.read().await;
    let mut all_plans = s.store.plan_store.list_for_session(&session_id);
    // Include plans from subagent sessions. DashMap::get returns a Ref
    // guard; deref to &Vec<String> for iteration.
    if let Some(children_ref) = s.store.session_children.get(&session_id) {
        for child_id in children_ref.value() {
            all_plans.extend(s.store.plan_store.list_for_session(child_id));
        }
    }
    // Sort by timestamp desc
    all_plans.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    let plans: Vec<Value> = all_plans
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "session_id": p.session_id,
                "title": p.title,
                "timestamp": p.timestamp,
            })
        })
        .collect();
    Json(Value::Array(plans))
}

// ── Query API endpoints (Phase B3) ──────────────────────────────────

/// GET /api/sessions/{session_id}/synopsis
pub async fn get_session_synopsis(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    log_event(
        "api",
        &format!("GET /api/sessions/{}/synopsis", short_id(&session_id)),
    );
    let s = state.read().await;
    let synopsis = s
        .store
        .event_store
        .query_session_synopsis(&session_id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(serde_json::to_value(synopsis).unwrap_or(json!({}))))
}

/// GET /api/sessions/{session_id}/tool-journey
pub async fn get_tool_journey(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    log_event(
        "api",
        &format!("GET /api/sessions/{}/tool-journey", short_id(&session_id)),
    );
    let s = state.read().await;
    let journey = s.store.event_store.query_tool_journey(&session_id).await;
    Json(serde_json::to_value(journey).unwrap_or(json!([])))
}

/// GET /api/sessions/{session_id}/file-impact
pub async fn get_file_impact(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    log_event(
        "api",
        &format!("GET /api/sessions/{}/file-impact", short_id(&session_id)),
    );
    let s = state.read().await;
    let impact = s.store.event_store.query_file_impact(&session_id).await;
    Json(serde_json::to_value(impact).unwrap_or(json!([])))
}

/// GET /api/sessions/{session_id}/errors
pub async fn get_session_errors(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Json<Value> {
    log_event(
        "api",
        &format!("GET /api/sessions/{}/errors", short_id(&session_id)),
    );
    let s = state.read().await;
    let errors = s.store.event_store.query_session_errors(&session_id).await;
    Json(serde_json::to_value(errors).unwrap_or(json!([])))
}

#[derive(Deserialize)]
pub struct DaysQuery {
    #[serde(default = "default_days")]
    pub days: u32,
}

fn default_days() -> u32 {
    7
}

/// GET /api/insights/pulse?days=7
pub async fn get_project_pulse(
    State(state): State<SharedState>,
    Query(query): Query<DaysQuery>,
) -> Json<Value> {
    log_event(
        "api",
        &format!("GET /api/insights/pulse?days={}", query.days),
    );
    let s = state.read().await;
    let pulse = s.store.event_store.query_project_pulse(query.days).await;
    Json(serde_json::to_value(pulse).unwrap_or(json!([])))
}

#[derive(Deserialize)]
pub struct EvolutionQuery {
    #[serde(default = "default_evolution_days")]
    pub days: u32,
}

fn default_evolution_days() -> u32 {
    30
}

/// GET /api/insights/tool-evolution?days=30
pub async fn get_tool_evolution(
    State(state): State<SharedState>,
    Query(query): Query<EvolutionQuery>,
) -> Json<Value> {
    log_event(
        "api",
        &format!("GET /api/insights/tool-evolution?days={}", query.days),
    );
    let s = state.read().await;
    let evolution = s.store.event_store.query_tool_evolution(query.days).await;
    Json(serde_json::to_value(evolution).unwrap_or(json!([])))
}

/// GET /api/insights/efficiency
pub async fn get_session_efficiency_insights(State(state): State<SharedState>) -> Json<Value> {
    log_event("api", "GET /api/insights/efficiency");
    let s = state.read().await;
    let efficiency = s.store.event_store.query_session_efficiency().await;
    Json(serde_json::to_value(efficiency).unwrap_or(json!([])))
}

#[derive(Deserialize)]
pub struct ProjectQuery {
    pub project: String,
}

/// GET /api/agent/project-context?project=X
pub async fn get_agent_project_context(
    State(state): State<SharedState>,
    Query(query): Query<ProjectQuery>,
) -> Json<Value> {
    log_event(
        "api",
        &format!("GET /api/agent/project-context?project={}", query.project),
    );
    let s = state.read().await;
    let context = s
        .store
        .event_store
        .query_project_context(&query.project, 5)
        .await;
    Json(serde_json::to_value(context).unwrap_or(json!([])))
}

/// GET /api/agent/recent-files?project=X
pub async fn get_agent_recent_files(
    State(state): State<SharedState>,
    Query(query): Query<ProjectQuery>,
) -> Json<Value> {
    log_event(
        "api",
        &format!("GET /api/agent/recent-files?project={}", query.project),
    );
    let s = state.read().await;
    let files = s
        .store
        .event_store
        .query_recent_files(&query.project, 5)
        .await;
    Json(serde_json::to_value(files).unwrap_or(json!([])))
}

#[derive(Deserialize)]
pub struct ProductivityQuery {
    #[serde(default = "default_evolution_days")]
    pub days: u32,
}

/// GET /api/insights/productivity?days=30
pub async fn get_productivity(
    State(state): State<SharedState>,
    Query(query): Query<ProductivityQuery>,
) -> Json<Value> {
    log_event(
        "api",
        &format!("GET /api/insights/productivity?days={}", query.days),
    );
    let s = state.read().await;
    let hourly = s
        .store
        .event_store
        .query_productivity_by_hour(query.days)
        .await;
    Json(serde_json::to_value(hourly).unwrap_or(json!([])))
}

// ── Token Usage ──────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct TokenUsageQuery {
    /// Filter to last N days
    pub days: Option<u32>,
    /// Filter to a single session
    pub session_id: Option<String>,
    /// Pricing model: sonnet (default), opus, haiku
    #[serde(default = "default_pricing_model")]
    pub model: String,
}

fn default_pricing_model() -> String {
    "sonnet".to_string()
}

/// GET /api/insights/token-usage?days=7&model=sonnet
///
/// Returns token usage summary with cost estimates.
/// Includes per-session breakdown sorted by output tokens.
pub async fn get_token_usage(
    State(state): State<SharedState>,
    Query(query): Query<TokenUsageQuery>,
) -> Json<Value> {
    log_event(
        "api",
        &format!(
            "GET /api/insights/token-usage?days={:?}&session_id={:?}&model={}",
            query.days, query.session_id, query.model
        ),
    );
    let s = state.read().await;
    let result = s
        .store
        .event_store
        .query_token_usage(query.days, query.session_id.as_deref(), &query.model)
        .await;
    Json(serde_json::to_value(result).unwrap_or(json!({})))
}

/// GET /api/insights/token-usage/daily?days=30
///
/// Returns daily token usage trend.
pub async fn get_daily_token_usage(
    State(state): State<SharedState>,
    Query(query): Query<DaysQuery>,
) -> Json<Value> {
    log_event(
        "api",
        &format!("GET /api/insights/token-usage/daily?days={}", query.days),
    );
    let s = state.read().await;
    let result = s
        .store
        .event_store
        .query_daily_token_usage(Some(query.days))
        .await;
    Json(serde_json::to_value(result).unwrap_or(json!([])))
}

// ── Agent Tool Definitions (Phase B5) ────────────────────────────────

/// GET /api/agent/tools
///
/// Returns tool definitions for the agentic query endpoints.
/// Agents can discover these tools and call the corresponding endpoints.
/// Format follows MCP-style tool definitions with JSON Schema parameters.
pub async fn get_agent_tools() -> Json<Value> {
    log_event("api", "GET /api/agent/tools");
    Json(json!([
        {
            "name": "project_context",
            "description": "Get the last 5 sessions for a project. Use this to pick up where the last agent left off.",
            "endpoint": "/api/agent/project-context",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "Project ID to query" }
                },
                "required": ["project"]
            }
        },
        {
            "name": "recent_files",
            "description": "Get files modified in recent sessions for a project. Focus on active files, not the whole repo.",
            "endpoint": "/api/agent/recent-files",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "Project ID to query" }
                },
                "required": ["project"]
            }
        },
        {
            "name": "session_synopsis",
            "description": "Get a synopsis of a session: goal, journey, outcome, top tools, error count.",
            "endpoint": "/api/sessions/{session_id}/synopsis",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID" }
                },
                "required": ["session_id"]
            }
        },
        {
            "name": "tool_journey",
            "description": "Get the sequence of tools used in a session with file targets. Understand the agent's strategy.",
            "endpoint": "/api/sessions/{session_id}/tool-journey",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID" }
                },
                "required": ["session_id"]
            }
        },
        {
            "name": "file_impact",
            "description": "Get files read vs. written in a session. Understand the blast radius of changes.",
            "endpoint": "/api/sessions/{session_id}/file-impact",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID" }
                },
                "required": ["session_id"]
            }
        },
        {
            "name": "project_pulse",
            "description": "Get activity per project over the last N days. See which projects are actively being worked on.",
            "endpoint": "/api/insights/pulse",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "days": { "type": "integer", "description": "Days to look back (default: 7)", "default": 7 }
                }
            }
        },
        {
            "name": "session_errors",
            "description": "Get errors from a session with timestamps. Understand what went wrong and when.",
            "endpoint": "/api/sessions/{session_id}/errors",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "session_id": { "type": "string", "description": "Session ID" }
                },
                "required": ["session_id"]
            }
        },
        {
            "name": "productivity_by_hour",
            "description": "Get event density by hour of day. Understand when deep agent work happens.",
            "endpoint": "/api/insights/productivity",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "days": { "type": "integer", "description": "Days to look back (default: 30)", "default": 30 }
                }
            }
        },
        {
            "name": "token_usage",
            "description": "Get token usage and estimated cost across all sessions. Shows input/output/cache tokens and cost breakdown. Filter by days or session_id. Returns per-session breakdown sorted by output tokens.",
            "endpoint": "/api/insights/token-usage",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "days": { "type": "integer", "description": "Only include last N days" },
                    "session_id": { "type": "string", "description": "Filter to a single session" },
                    "model": { "type": "string", "description": "Pricing model: sonnet (default), opus, haiku", "default": "sonnet" }
                }
            }
        },
        {
            "name": "daily_token_usage",
            "description": "Get daily token usage trend — how many tokens were used each day with cost estimates.",
            "endpoint": "/api/insights/token-usage/daily",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "days": { "type": "integer", "description": "Days to look back (default: 7)", "default": 7 }
                }
            }
        },
        {
            "name": "search",
            "description": "Full-text search across past sessions. Find how previous agents approached similar problems, what files they changed, and what strategies worked. Returns sessions ranked by relevance with matching event snippets. Use synopsis and tool_journey on the returned session IDs for deeper investigation.",
            "endpoint": "/api/agent/search",
            "method": "GET",
            "parameters": {
                "type": "object",
                "properties": {
                    "q": { "type": "string", "description": "Natural language search query" },
                    "project": { "type": "string", "description": "Optional project ID to filter results" },
                    "days": { "type": "integer", "description": "Days to look back (default: 30)", "default": 30 },
                    "limit": { "type": "integer", "description": "Max sessions to return (default: 5)", "default": 5 }
                },
                "required": ["q"]
            }
        }
    ]))
}

// ── FTS5 search endpoint ────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SearchQuery {
    pub q: Option<String>,
    #[serde(default = "default_search_limit")]
    pub limit: usize,
    pub session_id: Option<String>,
}

fn default_search_limit() -> usize {
    20
}

/// GET /api/search?q=<query>&limit=20&session_id=<optional>
///
/// Full-text search over indexed events using SQLite FTS5.
pub async fn search_events(
    State(state): State<SharedState>,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let q = match &query.q {
        Some(q) if !q.trim().is_empty() => q.trim().to_string(),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "missing or empty 'q' parameter"})),
            ));
        }
    };

    log_event(
        "api",
        &format!(
            "GET /api/search?q={}",
            crate::logging::truncate_at_char_boundary(&q, 50)
        ),
    );

    let s = state.read().await;
    let results = s
        .store
        .event_store
        .search_fts(&q, query.limit, query.session_id.as_deref())
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("search failed: {e}")})),
            )
        })?;

    Ok(Json(serde_json::to_value(results).unwrap_or(json!([]))))
}

// ── Agentic search endpoint ──────────────────────────────────────────

#[derive(Deserialize)]
pub struct AgentSearchQuery {
    pub q: Option<String>,
    #[serde(default = "default_agent_search_limit")]
    pub limit: usize,
    pub project: Option<String>,
    #[serde(default = "default_agent_search_days")]
    pub days: u32,
}

fn default_agent_search_limit() -> usize {
    5
}

fn default_agent_search_days() -> u32 {
    30
}

/// GET /api/agent/search?q=<query>&project=<optional>&days=30&limit=5
///
/// Session-grouped full-text search for agents. Returns sessions ranked by
/// relevance with matching event snippets and pointers to synopsis/journey
/// endpoints for deeper investigation.
pub async fn agent_search(
    State(state): State<SharedState>,
    Query(query): Query<AgentSearchQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let q = match &query.q {
        Some(q) if !q.trim().is_empty() => q.trim().to_string(),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error": "missing or empty 'q' parameter"})),
            ));
        }
    };

    log_event(
        "api",
        &format!(
            "GET /api/agent/search?q={}{}",
            &q[..q.len().min(50)],
            query
                .project
                .as_ref()
                .map(|p| format!("&project={p}"))
                .unwrap_or_default()
        ),
    );

    let s = state.read().await;

    // Search with a higher event limit — we'll group by session
    let event_limit = query.limit * 10;
    let results = s
        .store
        .event_store
        .search_fts(&q, event_limit, None)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("search failed: {e}")})),
            )
        })?;

    // Collect project filter info
    let project_filter = query.project.clone();
    let session_projects = s.store.session_projects.clone();
    let session_project_names = s.store.session_project_names.clone();

    // Collect session metadata for enrichment. DashMap iteration gives
    // RefMulti guards; `.key()` and `.value()` extract the pair.
    let mut session_labels: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut session_event_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for entry in s.store.projections.iter() {
        if let Some(label) = entry.value().label() {
            session_labels.insert(entry.key().clone(), label.to_string());
        }
        session_event_counts.insert(entry.key().clone(), entry.value().event_count());
    }

    // Group results by session
    let mut session_groups: std::collections::HashMap<
        String,
        Vec<&open_story_store::queries::FtsSearchResult>,
    > = std::collections::HashMap::new();
    for result in &results {
        session_groups
            .entry(result.session_id.clone())
            .or_default()
            .push(result);
    }

    // Build session-level results
    let mut session_results: Vec<Value> = session_groups
        .into_iter()
        .filter_map(|(sid, events)| {
            // Project filter: skip sessions not matching the requested project
            if let Some(ref proj) = project_filter {
                let session_project = session_projects.get(&sid)?;
                if !session_project.value().contains(proj) {
                    return None;
                }
            }

            // Session-level relevance = min rank (FTS5 rank is negative, more negative = more relevant)
            let best_rank = events.iter().map(|e| e.rank).fold(0.0f64, f64::min);

            let matching_events: Vec<Value> = events
                .iter()
                .take(3)
                .map(|e| {
                    json!({
                        "event_id": e.event_id,
                        "rank": e.rank,
                        "snippet": e.snippet,
                        "record_type": e.record_type,
                    })
                })
                .collect();

            let project_name = session_project_names.get(&sid).map(|r| r.value().clone());
            let project_id = session_projects.get(&sid).map(|r| r.value().clone());
            let label = session_labels.get(&sid);
            let event_count = session_event_counts.get(&sid).copied().unwrap_or(0);

            Some(json!({
                "session_id": sid,
                "label": label,
                "project_id": project_id,
                "project_name": project_name,
                "event_count": event_count,
                "relevance_rank": best_rank,
                "matching_events": matching_events,
                "synopsis_url": format!("/api/sessions/{sid}/synopsis"),
                "tool_journey_url": format!("/api/sessions/{sid}/tool-journey"),
            }))
        })
        .collect();

    // Sort by rank (more negative = more relevant, so ascending sort)
    session_results.sort_by(|a, b| {
        let rank_a = a["relevance_rank"].as_f64().unwrap_or(0.0);
        let rank_b = b["relevance_rank"].as_f64().unwrap_or(0.0);
        rank_a
            .partial_cmp(&rank_b)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    session_results.truncate(query.limit);

    Ok(Json(json!({
        "query": q,
        "results": session_results,
        "total_events_searched": results.len(),
    })))
}

// ── Records endpoint (WireRecords from projections) ─────────────────

/// Query parameters for `GET /api/sessions/{id}/records`.
///
/// Default behavior (no params) returns every record for the session as a
/// flat JSON array — preserved so the existing callers in SessionTimeline
/// and TurnCard keep working unchanged. When either `limit` or
/// `before_seq` is supplied, the response is paginated:
///   - records are sorted by `seq` ascending,
///   - filtered to `seq < before_seq` if `before_seq` is given,
///   - then the most-recent `limit` records are returned (oldest-first
///     within the window so the UI can prepend on scroll-up).
///
/// Use `before_seq = response[0].seq` to fetch the next page upward.
#[derive(Deserialize)]
pub struct SessionRecordsQuery {
    pub limit: Option<usize>,
    pub before_seq: Option<u64>,
}

const DEFAULT_RECORDS_LIMIT: usize = 500;
const MAX_RECORDS_LIMIT: usize = 2000;

/// GET /api/sessions/{session_id}/records
///
/// Returns session events as WireRecords read directly from the EventStore.
///
/// This is the same format the Timeline renders — includes depth,
/// parent_uuid, and truncation metadata. Returns empty array if the
/// session has no events.
///
/// Reads from `event_store.session_events()` (the single source of truth)
/// rather than any in-memory cache, so any event persisted to the store
/// is visible here regardless of which ingest path wrote it.
pub async fn get_session_records(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
    Query(query): Query<SessionRecordsQuery>,
) -> Json<Value> {
    use open_story_views::from_cloud_event::from_cloud_event;
    use open_story_views::unified::RecordBody;
    use open_story_views::wire_record::{TRUNCATION_THRESHOLD, WireRecord, truncate_payload};

    let paginated = query.limit.is_some() || query.before_seq.is_some();
    log_event(
        "api",
        &format!(
            "GET /api/sessions/{}/records{}",
            short_id(&session_id),
            if paginated {
                format!(
                    " (limit={:?} before_seq={:?})",
                    query.limit, query.before_seq
                )
            } else {
                String::new()
            }
        ),
    );
    let s = state.read().await;

    let events = if paginated {
        // Chunk-walk backward from the cursor instead of loading the whole
        // session (measured: full-load-then-slice cost ~0.9 s per page on a
        // 19k-event session). Events fan out to ≥1 ViewRecords, so one
        // chunk usually fills the page; events that translate to zero
        // records trigger another round so a mid-history page is never
        // accidentally short (the client reads a short page as
        // end-of-history).
        let limit = query
            .limit
            .unwrap_or(DEFAULT_RECORDS_LIMIT)
            .clamp(1, MAX_RECORDS_LIMIT);
        let mut collected: Vec<Value> = Vec::new();
        let mut record_estimate = 0usize;
        let mut cursor = query.before_seq;
        loop {
            let chunk = s
                .store
                .event_store
                .session_events_before(&session_id, cursor, limit)
                .await
                .unwrap_or_default();
            if chunk.is_empty() {
                break;
            }
            let exhausted = chunk.len() < limit;
            cursor = chunk
                .first()
                .and_then(|e| e.get("data"))
                .and_then(|d| d.get("seq"))
                .and_then(|v| v.as_u64());
            record_estimate += chunk
                .iter()
                .filter(|e| {
                    serde_json::from_value::<open_story_core::cloud_event::CloudEvent>(
                        (*e).clone(),
                    )
                    .map(|ce| !from_cloud_event(&ce).is_empty())
                    .unwrap_or(false)
                })
                .count();
            collected.splice(0..0, chunk);
            if record_estimate >= limit || exhausted {
                break;
            }
        }
        collected
    } else {
        s.store
            .event_store
            .session_events(&session_id)
            .await
            .unwrap_or_default()
    };

    // Build parent_map from raw events — one entry per stored CloudEvent.
    // Fan-out ViewRecords (e.g., parallel tool_use blocks) inherit the
    // same parent_uuid via suffix stripping at lookup time.
    let mut parent_map: HashMap<String, Option<String>> = HashMap::with_capacity(events.len());
    for event in &events {
        let id = match event.get("id").and_then(|v| v.as_str()) {
            Some(id) => id.to_string(),
            None => continue,
        };
        let parent = event
            .get("data")
            .and_then(|d| d.get("agent_payload"))
            .and_then(|ap| ap.get("parent_uuid"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        parent_map.insert(id, parent);
    }

    // A page may not contain a record's ancestors, so the local parent walk
    // can under-count depth at page edges. The projection maintains global
    // depths incrementally — prefer it, fall back to the local walk.
    let projection_depth = |id: &str| -> Option<u16> {
        let base_id = id.split(':').next().unwrap_or(id);
        s.store
            .projections
            .get(&session_id)
            .map(|p| p.node_depth(base_id))
    };

    // Depth: walk the parent chain. Capped at 64 to bound cost on
    // pathological inputs (production trees are shallow).
    fn depth_of(id: &str, parent_map: &HashMap<String, Option<String>>) -> u16 {
        // Strip fan-out suffix: "evt-1:2" → "evt-1"
        let base_id = id.split(':').next().unwrap_or(id);
        let mut depth: u16 = 0;
        let mut current = match parent_map.get(base_id).and_then(|p| p.as_deref()) {
            Some(p) => p.to_string(),
            None => return 0,
        };
        for _ in 0..64 {
            depth += 1;
            match parent_map.get(current.as_str()).and_then(|p| p.as_deref()) {
                Some(next) => current = next.to_string(),
                None => return depth,
            }
        }
        depth
    }

    let mut wires: Vec<WireRecord> = Vec::new();
    for event in &events {
        let ce =
            match serde_json::from_value::<open_story_core::cloud_event::CloudEvent>(event.clone())
            {
                Ok(ce) => ce,
                Err(_) => continue,
            };
        for vr in from_cloud_event(&ce) {
            // Parent lookup uses base id (strip fan-out suffix).
            let base_id = vr.id.split(':').next().unwrap_or(&vr.id).to_string();
            let parent_uuid = parent_map.get(&base_id).and_then(|p| p.clone());
            let depth = projection_depth(&vr.id)
                .unwrap_or_else(|| depth_of(&vr.id, &parent_map));

            // Truncation: same rule as the pre-refactor to_wire_record.
            let (truncated, payload_bytes) = match &vr.body {
                RecordBody::ToolResult(tr) => match &tr.output {
                    Some(output) => {
                        let result = truncate_payload(output, TRUNCATION_THRESHOLD);
                        (result.truncated, result.original_bytes as u64)
                    }
                    None => (false, 0),
                },
                _ => (false, 0),
            };

            wires.push(WireRecord {
                record: vr,
                depth,
                parent_uuid,
                truncated,
                payload_bytes,
            });
        }
    }

    // Events come out of session_events() sorted by (seq, time) per the
    // store contract. Pagination operates on `seq` directly.
    if paginated {
        let limit = query
            .limit
            .unwrap_or(DEFAULT_RECORDS_LIMIT)
            .clamp(1, MAX_RECORDS_LIMIT);

        // Filter by before_seq if provided.
        if let Some(before) = query.before_seq {
            wires.retain(|w| w.record.seq < before);
        }

        // Keep the most-recent `limit` records, ordered oldest-first
        // (so the UI can prepend on scroll-up).
        wires.sort_by_key(|w| w.record.seq);
        let total = wires.len();
        if total > limit {
            wires = wires.split_off(total - limit);
        }
    }

    let records: Vec<Value> = wires
        .into_iter()
        .filter_map(|w| serde_json::to_value(w).ok())
        .collect();

    Json(Value::Array(records))
}

// ── Session Lifecycle endpoints (Phase A4) ──────────────────────────

/// DELETE /api/sessions/{session_id}
///
/// Removes a session and all its events, patterns, and plans from SQLite.
/// Also clears in-memory projections and caches.
pub async fn delete_session(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<Json<Value>, StatusCode> {
    log_event(
        "api",
        &format!("DELETE /api/sessions/{}", short_id(&session_id)),
    );
    let s = state.write().await;

    let deleted = s
        .store
        .event_store
        .delete_session(&session_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if deleted == 0 && !s.store.projections.contains_key(&session_id) {
        return Err(StatusCode::NOT_FOUND);
    }

    // Clean up in-memory state
    s.store.projections.remove(&session_id);
    s.store.detected_patterns.remove(&session_id);
    // full_payloads is keyed on (session_id, event_id) — walk to prune.
    let to_drop: Vec<(String, String)> = s
        .store
        .full_payloads
        .iter()
        .filter_map(|e| {
            if e.key().0 == session_id {
                Some(e.key().clone())
            } else {
                None
            }
        })
        .collect();
    for k in to_drop {
        s.store.full_payloads.remove(&k);
    }
    s.store.session_projects.remove(&session_id);
    s.store.session_project_names.remove(&session_id);

    Ok(Json(json!({
        "status": "deleted",
        "session_id": session_id,
        "events_deleted": deleted,
    })))
}

/// GET /api/sessions/{session_id}/export
///
/// Returns all events for a session as newline-delimited JSON (JSONL).
/// Content-Type: application/x-ndjson for proper JSONL handling.
pub async fn export_session(
    State(state): State<SharedState>,
    AxumPath(session_id): AxumPath<String>,
) -> Result<
    (
        StatusCode,
        [(axum::http::header::HeaderName, &'static str); 1],
        String,
    ),
    StatusCode,
> {
    log_event(
        "api",
        &format!("GET /api/sessions/{}/export", short_id(&session_id)),
    );
    let s = state.read().await;

    let jsonl = s
        .store
        .event_store
        .export_session_jsonl(&session_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if jsonl.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok((
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/x-ndjson")],
        jsonl,
    ))
}
