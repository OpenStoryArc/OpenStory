//! `ui_control` — the agent-in-UI WRITE seam as a first-class MCP tool.
//!
//! Lets an agent DRIVE the OpenStory dashboard (navigate / present / toggle /
//! set) by POSTing a control intent to `/api/control`, which the server
//! broadcasts to every open dashboard. It steers only what the mirror SHOWS —
//! never the observed sources ("drive the mirror, never the watched").
//!
//! The body assembly is pure (`build_control_body`) so the vocabulary contract
//! is unit-tested independently of the HTTP round-trip.

use serde_json::{json, Value};

/// Full verb set the dashboard's `interpretControl` accepts (plus aliases).
/// Kept in the schema description so agents discover the map without grepping.
pub const CONTROL_VERBS: &str = "open_view | focus_event | navigate_to | present | announce | highlight | query | filter | set_filter | toggle | set";

pub fn ui_control_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "description": format!(
                    "{CONTROL_VERBS}. \
                     open_view: navigate (route hash OR structured view/sessionId/detailView/eventId/…). \
                     focus_event: one event in Explore/Story; spotlight:true = full-screen. \
                     navigate_to: HIGH-LEVEL hand — {{kind, id, sessionId?, canvasMode?, details?, view?}} plans a multi-step drive (any event, any canvas mode + session click). Prefer this for click-parity. \
                     present|announce|highlight: banner / title card. \
                     query|filter: fleet facets. \
                     toggle: canvas.mode, story.sort, theme, …. \
                     set: story.details, canvas.select_session, scatter.brush, …."
                ),
            },
            "params": {
                "type": "object",
                "description": "action params. navigate_to: {kind: event|session|file|person|project|turn|sentence|canvas|subagent, id, sessionId?, eventId?, view?, details?, canvasMode?, spotlight?}. \
                                open_view: {route} OR {view, sessionId?, …}. focus_event: {sessionId, eventId, view?, spotlight?, clipAt?}. \
                                set: {target: canvas.select_session|story.details|…, …fields}. toggle: {target, value}."
            }
        },
        "required": ["action"]
    })
}

/// `navigate_to` — primary agent hand for full click-parity.
/// Posts `action: navigate_to` so the dashboard plans + runs the control sequence.
pub fn navigate_to_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "kind": {
                "type": "string",
                "description": "event | session | file | person | project | turn | sentence | canvas | subagent | day | facet | reel"
            },
            "day": { "type": "string", "description": "YYYY-MM-DD heatmap day → explore filter (kind=day or with heatmap)" },
            "agent": { "type": "string", "description": "canvas flow agent chip (with canvasMode=flow)" },
            "facet": {
                "type": "string",
                "description": "kind=facet dimension: project|host|user|branch|status|agent|day (optional when id is chip-id facet-host-… or host:…)"
            },
            "groupBy": { "type": "string", "description": "canvas hierarchy: user|day|agent|status|host|project" },
            "metric": { "type": "string", "description": "canvas sunburst/treemap size: events|tokens" },
            "id": {
                "type": "string",
                "description": "Entity id (event UUID, session UUID, file path, user, project, …). kind=facet: chip-id facet-host-… or value with facet=. For kind=canvas may be omitted if only switching mode."
            },
            "sessionId": { "type": "string", "description": "Required for kind=event (and helpful for turn/sentence)" },
            "eventId": { "type": "string", "description": "Optional; for turn/sentence if id is not the event id" },
            "view": { "type": "string", "description": "story | explore (events default story)" },
            "details": { "type": "boolean", "description": "Expand Story ▾ details (sentence depth)" },
            "evalOpen": { "type": "boolean", "description": "Expand Story eval-apply under the turn (?eval=1)" },
            "eventsOpen": { "type": "boolean", "description": "Expand Story event-id list (?events=1)" },
            "expandAll": { "type": "boolean", "description": "details + eval + events + all apply outputs" },
            "applyOpen": {
                "description": "Per-apply output expand inside eval-apply: true|\"all\" or 0-based index / array of indices (?apply=0,2 or ?apply=all)"
            },
            "expandKeys": {
                "description": "Board hierarchy expand keys (g:group / p:group:project) — same as clicking a circle to drill",
                "type": "array",
                "items": { "type": "string" }
            },
            "canvasMode": {
                "type": "string",
                "description": "sunburst|board|treemap|gantt|scatter|flow|tool-adjacency|agent-project|durations|heatmap"
            },
            "spotlight": { "type": "boolean", "description": "Full-screen event presentation" },
            "autoplay": { "type": "boolean", "description": "kind=reel only: start playback on arrival" }
        },
        "required": ["kind"]
    })
}

pub async fn navigate_to(api_base: &str, args: Value) -> Result<Value, String> {
    let kind = args.get("kind").and_then(|v| v.as_str()).unwrap_or("").trim();
    if kind.is_empty() {
        return Err("navigate_to requires `kind` (event|session|file|person|project|turn|sentence|canvas|subagent|day|facet|reel)".to_string());
    }
    // Flatten tool args into navigate_to params (id optional for canvas-only mode).
    let mut params = args.clone();
    let mut resolved_session: Option<String> = None;
    if let Some(obj) = params.as_object_mut() {
        // keep kind inside params for the UI planner
        obj.insert("kind".into(), json!(kind));
        if !obj.contains_key("id") {
            obj.insert("id".into(), json!(kind));
        }
        // P4: event without sessionId → resolve via FTS (pure join of search hits).
        let needs_session = matches!(kind, "event" | "turn" | "sentence");
        let has_session = obj
            .get("sessionId")
            .and_then(|v| v.as_str())
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        if needs_session && !has_session {
            let eid = obj
                .get("id")
                .or_else(|| obj.get("eventId"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            if !eid.is_empty() {
                if let Some(sid) = resolve_session_for_event(api_base, &eid).await {
                    obj.insert("sessionId".into(), json!(sid));
                    resolved_session = Some(sid);
                }
            }
        }
    }
    let result = ui_control(
        api_base,
        json!({ "action": "navigate_to", "params": params }),
    )
    .await?;
    // Best-effort where_is_user so the agent sees land without a second call.
    let ui = where_is_user(api_base, json!({})).await.ok();
    Ok(json!({
        "ok": result.get("ok").and_then(|v| v.as_bool()).unwrap_or(true),
        "delivered": result.get("delivered"),
        "action": "navigate_to",
        "kind": kind,
        "params": params,
        "resolved_sessionId": resolved_session,
        "ui_state": ui,
        "hint": "Prefer navigate_to for any event / canvas graph click. Event ids auto-resolve sessionId via search when omitted."
    }))
}

/// Look up which session owns an event id (FTS). Pure join of search results.
async fn resolve_session_for_event(api_base: &str, event_id: &str) -> Option<String> {
    let base = api_base.trim_end_matches('/');
    let url = format!(
        "{}/api/search?q={}&limit=5",
        base,
        urlencoding_loose(event_id)
    );
    let resp = reqwest::Client::new().get(&url).send().await.ok()?;
    let body: Value = resp.json().await.ok()?;
    // Accept { results: [...] } or bare array
    let hits = body
        .get("results")
        .and_then(|v| v.as_array())
        .or_else(|| body.as_array())?;
    // Prefer exact event_id match
    for h in hits {
        let eid = h
            .get("event_id")
            .or_else(|| h.get("id"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let sid = h.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
        if eid == event_id && !sid.is_empty() {
            return Some(sid.to_string());
        }
    }
    hits.first()
        .and_then(|h| h.get("session_id"))
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn urlencoding_loose(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}

/// Pure: validate + assemble the `/api/control` request body. Errors if
/// `action` is missing/blank; defaults `params` to `{}`; stamps the issuer so
/// the dashboard's "▸ driven by" indicator names the agent. Any non-blank
/// action is accepted at the HTTP boundary — the UI's interpretControl is the
/// open vocabulary sink (unknown actions no-op on the dashboard).
pub fn build_control_body(args: &Value) -> Result<Value, String> {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("").trim();
    if action.is_empty() {
        return Err(format!("ui_control requires `action` ({CONTROL_VERBS})"));
    }
    let params = args.get("params").cloned().unwrap_or_else(|| json!({}));
    Ok(json!({ "action": action, "params": params, "issuer": "agent:mcp" }))
}

/// POST the control intent to `{api_base}/api/control`. Returns the server's
/// `{ ok, action, delivered }` (delivered = how many dashboards received it).
pub async fn ui_control(api_base: &str, args: Value) -> Result<Value, String> {
    if api_base.trim().is_empty() {
        return Err("ui_control unavailable: the MCP has no API base configured (set OPENSTORY_API_URL)".to_string());
    }
    let body = build_control_body(&args)?;
    let url = format!("{}/api/control", api_base.trim_end_matches('/'));
    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("control POST failed: {e}"))?;
    resp.json::<Value>()
        .await
        .map_err(|e| format!("control response parse failed: {e}"))
}

// ── READ half: where_is_user (GET /api/ui-state) ───────────────────────────

pub fn where_is_user_schema() -> Value {
    json!({ "type": "object", "properties": {}, "additionalProperties": false })
}

/// Pure: turn the raw `ui_state` projection into an agent-friendly shape with a
/// one-line summary, so an agent can reason about "where is the user" without
/// digging. `Null` (no interaction recorded) → `present: false`.
///
/// Passes through HashRoute-parity fields the client reports (detailView,
/// eventId, filters, filePath, Live filters, searchQuery, optional spotlight /
/// present_message) so where_is_user can confirm a full drive — not just the
/// coarse tab. Privacy: navigation state only; no raw secrets or tool payloads.
pub fn summarize_ui_state(state: Value) -> Value {
    if state.is_null() {
        return json!({
            "present": false,
            "summary": "no interaction recorded yet — the user's position is unknown"
        });
    }
    let view = state.get("view").and_then(|v| v.as_str()).unwrap_or("unknown");
    let kind = state.get("kind").and_then(|v| v.as_str()).unwrap_or("view");
    let session_id = state.get("session_id").and_then(|v| v.as_str()).filter(|s| !s.is_empty());
    let at = state.get("at").and_then(|v| v.as_str()).unwrap_or("");

    // Accept both camelCase (client Interaction) and snake_case (any legacy).
    let str_field = |camel: &str, snake: &str| -> Option<String> {
        state
            .get(camel)
            .or_else(|| state.get(snake))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    };
    let event_id = str_field("eventId", "event_id");
    let detail_view = str_field("detailView", "detail_view");
    let file_path = str_field("filePath", "file_path");
    let search_query = str_field("searchQuery", "search_query");
    let user_filter = str_field("userFilter", "user_filter");
    let time_filter = str_field("timeFilter", "time_filter");
    let present_message = str_field("present_message", "present_message");
    let filters = state.get("filters").cloned().filter(|v| !v.is_null());
    let spotlight = state
        .get("spotlight")
        .and_then(|v| v.as_bool())
        .filter(|&b| b);
    // Layout eyes — glass geometry (viewport 0..1 targets). Client measures
    // DOM and posts on interactions; agents use this to aim the pen.
    let layout = state.get("layout").cloned().filter(|v| !v.is_null());
    // Pen eyes — bounded draw$ scene snapshot (ui.* ink the human/agent made).
    let pen = state.get("pen").cloned().filter(|v| !v.is_null());
    let annotate = state
        .get("annotate")
        .and_then(|v| v.as_bool())
        .or_else(|| pen.as_ref().and_then(|p| p.get("interactive").and_then(|v| v.as_bool())));
    let reel_id = str_field("reelId", "reel_id");

    let mut summary = match session_id {
        Some(sid) => format!("the user is on '{view}' viewing session {sid}"),
        None => format!("the user is on '{view}'"),
    };
    if let Some(ref dv) = detail_view {
        summary.push_str(&format!(" / {dv}"));
    }
    if let Some(ref eid) = event_id {
        summary.push_str(&format!(" focused on event {eid}"));
    }
    if let Some(ref fp) = file_path {
        summary.push_str(&format!(" file {fp}"));
    }
    if spotlight == Some(true) {
        summary.push_str(" (spotlight on)");
    }
    if let Some(ref msg) = present_message {
        let clip: String = msg.chars().take(40).collect();
        summary.push_str(&format!(" present: \"{clip}\""));
    }
    if let Some(ref lay) = layout {
        if let Some(focus) = lay.get("focus") {
            if let (Some(k), Some(id)) = (
                focus.get("kind").and_then(|v| v.as_str()),
                focus.get("id").and_then(|v| v.as_str()),
            ) {
                summary.push_str(&format!(" layout focus {k}:{id}"));
            }
        } else if let Some(n) = lay.get("targets").and_then(|t| t.as_array()).map(|a| a.len()) {
            if n > 0 {
                summary.push_str(&format!(" layout {n} target(s)"));
            }
        }
    }
    if let Some(ref p) = pen {
        let empty = p.get("empty").and_then(|v| v.as_bool()).unwrap_or(false);
        if empty {
            summary.push_str(" pen empty");
        } else if let Some(n) = p.get("stroke_count").and_then(|v| v.as_u64()) {
            summary.push_str(&format!(" pen {n} stroke(s)"));
            if let Some(label) = p.get("label").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                summary.push_str(&format!(" \"{label}\""));
            }
        }
        if p.get("interactive").and_then(|v| v.as_bool()) == Some(true) {
            summary.push_str(" annotating");
        }
    } else if annotate == Some(true) {
        summary.push_str(" annotating");
    }
    if let Some(ref rid) = reel_id {
        summary.push_str(&format!(" reel {rid}"));
    }

    json!({
        "present": true,
        "view": view,
        "kind": kind,
        "session_id": session_id,
        "event_id": event_id,
        "detail_view": detail_view,
        "file_path": file_path,
        "search_query": search_query,
        "user_filter": user_filter,
        "time_filter": time_filter,
        "filters": filters,
        "spotlight": spotlight,
        "present_message": present_message,
        "layout": layout,
        "pen": pen,
        "annotate": annotate,
        "reel_id": reel_id,
        "at": at,
        "summary": summary
    })
}

/// Pure: build the `subscribe_ui_state` stream notification payload from a
/// streamed `ui.*` frame. The frame is an `IngestBatch` value — `{ events:
/// [CloudEvent] }` — whose first event's `data` is an EventData envelope; the
/// interaction (`{kind, view, session_id?, at}`) rides in `data.raw`. We reach
/// the innermost body and reuse `summarize_ui_state` so a streamed frame matches
/// `where_is_user`. Tolerant of the earlier shapes: a bare CloudEvent (`data` =
/// EventData), a legacy flat `data`, and the bare inner body.
pub fn ui_state_notification(event: &Value) -> Value {
    // IngestBatch → first event's data; else a bare CloudEvent's data; else the
    // event itself (already the inner shape).
    let ce_data = event
        .get("events")
        .and_then(|e| e.get(0))
        .and_then(|ce| ce.get("data"))
        .or_else(|| event.get("data"))
        .unwrap_or(event);
    // EventData → unwrap `.raw` (the authored body); tolerate a flat shape.
    let inner = match ce_data.get("raw") {
        Some(raw) if !raw.is_null() => raw.clone(),
        _ => ce_data.clone(),
    };
    summarize_ui_state(inner)
}

/// GET `{api_base}/api/ui-state` → the current position (READ half of the seam).
pub async fn where_is_user(api_base: &str, _args: Value) -> Result<Value, String> {
    if api_base.trim().is_empty() {
        return Err("where_is_user unavailable: the MCP has no API base configured (set OPENSTORY_API_URL)".to_string());
    }
    let url = format!("{}/api/ui-state", api_base.trim_end_matches('/'));
    let resp = reqwest::Client::new()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("ui-state GET failed: {e}"))?;
    let body = resp.json::<Value>().await.map_err(|e| format!("ui-state parse failed: {e}"))?;
    Ok(summarize_ui_state(body.get("ui_state").cloned().unwrap_or(Value::Null)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_body_requires_an_action() {
        let err = build_control_body(&json!({ "params": { "route": "/canvas" } })).unwrap_err();
        assert!(err.contains("action"), "error should mention action: {err}");
    }

    #[test]
    fn build_body_rejects_blank_action() {
        assert!(build_control_body(&json!({ "action": "   " })).is_err());
    }

    #[test]
    fn build_body_assembles_action_params_issuer() {
        let body = build_control_body(&json!({
            "action": "toggle",
            "params": { "target": "canvas.mode", "value": "delegation" }
        }))
        .unwrap();
        assert_eq!(body["action"], "toggle");
        assert_eq!(body["params"]["target"], "canvas.mode");
        assert_eq!(body["params"]["value"], "delegation");
        assert_eq!(body["issuer"], "agent:mcp");
    }

    #[test]
    fn build_body_defaults_params_to_empty_object() {
        let body = build_control_body(&json!({ "action": "open_view" })).unwrap();
        assert_eq!(body["params"], json!({}));
    }

    #[test]
    fn summarize_null_state_is_not_present() {
        let s = summarize_ui_state(Value::Null);
        assert_eq!(s["present"], false);
        assert!(s["summary"].as_str().unwrap().contains("unknown"));
    }

    #[test]
    fn summarize_view_only_position() {
        let s = summarize_ui_state(json!({ "view": "overview", "kind": "navigate", "at": "2026-07-02T12:42:27Z" }));
        assert_eq!(s["present"], true);
        assert_eq!(s["view"], "overview");
        assert_eq!(s["session_id"], Value::Null);
        assert!(s["summary"].as_str().unwrap().contains("overview"));
    }

    #[test]
    fn summarize_names_the_session_when_present() {
        let s = summarize_ui_state(json!({ "view": "story", "kind": "navigate", "session_id": "abc123", "at": "t" }));
        assert_eq!(s["session_id"], "abc123");
        assert!(s["summary"].as_str().unwrap().contains("abc123"));
    }

    #[test]
    fn summarize_treats_empty_session_id_as_none() {
        let s = summarize_ui_state(json!({ "view": "overview", "session_id": "" }));
        assert_eq!(s["session_id"], Value::Null);
    }

    #[test]
    fn summarize_passes_through_hash_route_parity_fields() {
        // Client interactionFromRoute reports camelCase; agent needs full
        // where_is_user parity with bookmarkable UI state.
        let s = summarize_ui_state(json!({
            "view": "explore",
            "kind": "navigate",
            "session_id": "s1",
            "detailView": "conversation",
            "eventId": "e9",
            "filePath": "src/a.ts",
            "filters": { "agent": "grok" },
            "userFilter": "katie",
            "timeFilter": "today",
            "searchQuery": "auth",
            "spotlight": true,
            "at": "t"
        }));
        assert_eq!(s["present"], true);
        assert_eq!(s["event_id"], "e9");
        assert_eq!(s["detail_view"], "conversation");
        assert_eq!(s["file_path"], "src/a.ts");
        assert_eq!(s["filters"]["agent"], "grok");
        assert_eq!(s["user_filter"], "katie");
        assert_eq!(s["time_filter"], "today");
        assert_eq!(s["search_query"], "auth");
        assert_eq!(s["spotlight"], true);
        let summary = s["summary"].as_str().unwrap();
        assert!(summary.contains("conversation"), "summary: {summary}");
        assert!(summary.contains("e9"), "summary: {summary}");
        assert!(summary.contains("spotlight"), "summary: {summary}");
    }

    #[test]
    fn summarize_accepts_snake_case_aliases() {
        let s = summarize_ui_state(json!({
            "view": "explore",
            "session_id": "s1",
            "event_id": "evt-1",
            "detail_view": "events"
        }));
        assert_eq!(s["event_id"], "evt-1");
        assert_eq!(s["detail_view"], "events");
    }

    #[test]
    fn summarize_passes_through_layout_eyes() {
        let s = summarize_ui_state(json!({
            "view": "explore",
            "kind": "navigate",
            "session_id": "s1",
            "eventId": "e9",
            "layout": {
                "targets": [{
                    "kind": "event",
                    "id": "e9",
                    "rect": { "x": 0.2, "y": 0.3, "w": 0.4, "h": 0.1 }
                }],
                "focus": {
                    "kind": "event",
                    "id": "e9",
                    "rect": { "x": 0.2, "y": 0.3, "w": 0.4, "h": 0.1 }
                },
                "viewport": { "w": 1200, "h": 800 },
                "at": "t"
            },
            "at": "t"
        }));
        assert_eq!(s["layout"]["focus"]["id"], "e9");
        assert_eq!(s["layout"]["focus"]["rect"]["x"], 0.2);
        let summary = s["summary"].as_str().unwrap();
        assert!(summary.contains("layout focus event:e9"), "summary: {summary}");
    }

    #[test]
    fn summarize_passes_through_pen_eyes() {
        let s = summarize_ui_state(json!({
            "view": "draw",
            "kind": "navigate",
            "pen": {
                "stroke_count": 3,
                "empty": false,
                "label": "human",
                "kinds": { "path": 2, "text": 1 },
                "strokes": [],
                "truncated": false
            },
            "at": "t"
        }));
        assert_eq!(s["pen"]["stroke_count"], 3);
        assert_eq!(s["pen"]["label"], "human");
        let summary = s["summary"].as_str().unwrap();
        assert!(summary.contains("pen 3 stroke"), "summary: {summary}");
        assert!(summary.contains("human"), "summary: {summary}");
    }

    #[test]
    fn summarize_annotate_and_reel_id() {
        let s = summarize_ui_state(json!({
            "view": "reels",
            "kind": "navigate",
            "reelId": "reel-abc",
            "annotate": true,
            "pen": {
                "stroke_count": 0,
                "empty": true,
                "interactive": true,
                "strokes": []
            },
            "at": "t"
        }));
        assert_eq!(s["annotate"], true);
        assert_eq!(s["reel_id"], "reel-abc");
        let summary = s["summary"].as_str().unwrap();
        assert!(summary.contains("annotating"), "summary: {summary}");
        assert!(summary.contains("reel-abc"), "summary: {summary}");
    }

    #[test]
    fn build_body_accepts_full_verb_set() {
        for verb in [
            "open_view",
            "focus_event",
            "present",
            "announce",
            "highlight",
            "query",
            "filter",
            "set_filter",
            "toggle",
            "set",
        ] {
            let body = build_control_body(&json!({ "action": verb, "params": {} })).unwrap();
            assert_eq!(body["action"], verb, "verb {verb}");
            assert_eq!(body["issuer"], "agent:mcp");
        }
    }

    #[test]
    fn schema_documents_focus_event_and_full_verb_set() {
        let schema = ui_control_schema();
        let desc = schema["properties"]["action"]["description"]
            .as_str()
            .expect("action description");
        assert!(desc.contains("focus_event"), "schema must list focus_event");
        assert!(desc.contains("open_view"), "schema must list open_view");
        assert!(desc.contains("query"), "schema must list query");
        assert!(desc.contains("announce"), "schema must list present aliases");
        assert!(desc.contains("spotlight"), "schema should document spotlight");
    }

    #[test]
    fn notification_unwraps_a_cloudevent_data_field() {
        // as published on ui.* by 1c-1: a CloudEvent wrapping the interaction
        let event = json!({
            "specversion": "1.0", "type": "io.arc.event", "subtype": "interaction.navigate",
            "agent": "openstory-ui", "time": "t",
            "data": { "kind": "navigate", "view": "canvas", "session_id": "s1", "at": "t" }
        });
        let n = ui_state_notification(&event);
        assert_eq!(n["present"], true);
        assert_eq!(n["view"], "canvas");
        assert_eq!(n["session_id"], "s1");
    }

    #[test]
    fn notification_unwraps_ingestbatch_events_data_raw() {
        // Phase 1g-ii: the streamed frame is an IngestBatch (typed pump), so the
        // body is at events[0].data.raw.
        let batch = json!({
            "session_id": "openstory-ui",
            "project_id": "openstory-ui",
            "events": [{
                "specversion": "1.0", "type": "io.arc.event", "subtype": "interaction.navigate",
                "agent": "openstory-ui", "time": "t",
                "data": {
                    "raw": { "kind": "navigate", "view": "canvas", "session_id": "s7", "at": "t" },
                    "seq": 0, "session_id": "openstory-ui"
                }
            }]
        });
        let n = ui_state_notification(&batch);
        assert_eq!(n["present"], true);
        assert_eq!(n["view"], "canvas");
        assert_eq!(n["session_id"], "s7");
        assert_eq!(n["kind"], "navigate");
    }

    #[test]
    fn notification_unwraps_proper_cloudevent_eventdata_raw() {
        // Phase 1g: a proper CloudEvent — the body rides in data.raw (EventData).
        let event = json!({
            "specversion": "1.0", "type": "io.arc.event", "subtype": "interaction.select",
            "agent": "openstory-ui", "time": "t",
            "data": {
                "raw": { "kind": "select", "view": "explore", "session_id": "s9", "at": "t" },
                "seq": 0, "session_id": "openstory-ui"
            }
        });
        let n = ui_state_notification(&event);
        assert_eq!(n["present"], true);
        assert_eq!(n["view"], "explore");
        assert_eq!(n["session_id"], "s9");
        assert_eq!(n["kind"], "select");
    }

    #[test]
    fn notification_accepts_the_bare_inner_shape_too() {
        let n = ui_state_notification(&json!({ "kind": "navigate", "view": "story" }));
        assert_eq!(n["present"], true);
        assert_eq!(n["view"], "story");
    }

    #[test]
    fn notification_null_event_is_not_present() {
        assert_eq!(ui_state_notification(&Value::Null)["present"], false);
    }
}
