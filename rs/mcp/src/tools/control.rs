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

pub fn ui_control_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "action": {
                "type": "string",
                "description": "open_view | present | toggle | set | query"
            },
            "params": {
                "type": "object",
                "description": "action params — e.g. {route:'/canvas'} for open_view, \
                                {target:'canvas.mode', value:'delegation'} for toggle, \
                                {message, sessionIds, route} for present"
            }
        },
        "required": ["action"]
    })
}

/// Pure: validate + assemble the `/api/control` request body. Errors if
/// `action` is missing/blank; defaults `params` to `{}`; stamps the issuer so
/// the dashboard's "▸ driven by" indicator names the agent.
pub fn build_control_body(args: &Value) -> Result<Value, String> {
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("").trim();
    if action.is_empty() {
        return Err("ui_control requires `action` (open_view | present | toggle | set | query)".to_string());
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
    let summary = match session_id {
        Some(sid) => format!("the user is on '{view}' viewing session {sid}"),
        None => format!("the user is on '{view}'"),
    };
    json!({ "present": true, "view": view, "kind": kind, "session_id": session_id, "at": at, "summary": summary })
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
}
