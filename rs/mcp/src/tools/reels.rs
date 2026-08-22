//! Reel tools — save / list / play saved story sequences over the REST
//! surface (validation lives server-side, in one place).

use serde_json::{json, Value};

pub fn save_reel_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": {"type": "string", "description": "Reel title (shown in the Reels tab)"},
            "stops": {
                "type": "array",
                "description": "Ordered beats. Default kind=spotlight needs a REAL event. kind=title|diagram|image are interpretation slides (line required; event optional).",
                "items": {
                    "type": "object",
                    "properties": {
                        "sessionId": {"type": "string"},
                        "eventId": {"type": "string"},
                        "line": {"type": "string", "description": "Narration/caption text, written for the ear"},
                        "clipAt": {"type": "string", "description": "Optional camera crop: show text before this marker"},
                        "kind": {
                            "type": "string",
                            "description": "spotlight (default) | title | diagram | image"
                        },
                        "visual": {
                            "type": "object",
                            "description": "diagram: {kind:'labels', labels:string[]} or {kind:'toolJourney', sessionId}. image: {imageHref}.",
                            "properties": {
                                "kind": {"type": "string"},
                                "sessionId": {"type": "string"},
                                "labels": {"type": "array", "items": {"type": "string"}},
                                "imageHref": {"type": "string"},
                                "title": {"type": "string"}
                            }
                        }
                    },
                    "required": ["line"]
                }
            },
            "opener": {"type": "string", "description": "Optional BLUF title card shown and narrated BEFORE stop 1 — one breath stating what the story is and why it matters"},
            "closer": {"type": "string", "description": "Optional full-screen title-card line after the last stop"},
            "author": {"type": "string", "description": "Attribution (defaults to 'mcp')"}
        },
        "required": ["title", "stops"],
        "additionalProperties": false
    })
}

pub fn list_reels_schema() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": false})
}

pub fn play_reel_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"id": {"type": "string", "description": "Reel id from save_reel/list_reels"}},
        "required": ["id"],
        "additionalProperties": false
    })
}

/// Mirror `control.rs`'s api_base guard: reject a blank base up front (clear
/// error naming the missing env var) and trim a trailing '/' so joined URLs
/// never double up on slashes.
fn require_api_base(api_base: &str, tool: &str) -> Result<String, String> {
    if api_base.trim().is_empty() {
        return Err(format!(
            "{tool} unavailable: the MCP has no API base configured (set OPENSTORY_API_URL)"
        ));
    }
    Ok(api_base.trim_end_matches('/').to_string())
}

pub async fn save_reel(api_base: &str, mut args: Value) -> Result<Value, String> {
    let base = require_api_base(api_base, "save_reel")?;
    if args.get("author").and_then(|a| a.as_str()).unwrap_or("").is_empty() {
        args["author"] = json!("mcp");
    }
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/reels"))
        .json(&args)
        .send()
        .await
        .map_err(|e| format!("save_reel: {e}"))?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| format!("save_reel: {e}"))?;
    if status.as_u16() == 422 {
        // Not a transport error — surface the invalid stops so the agent re-searches.
        return Ok(body);
    }
    if !status.is_success() {
        return Err(format!("save_reel: HTTP {status}: {body}"));
    }
    Ok(body)
}

pub async fn list_reels(api_base: &str, _args: Value) -> Result<Value, String> {
    let base = require_api_base(api_base, "list_reels")?;
    reqwest::Client::new()
        .get(format!("{base}/api/reels"))
        .send()
        .await
        .map_err(|e| format!("list_reels: {e}"))?
        .json()
        .await
        .map_err(|e| format!("list_reels: {e}"))
}

pub async fn play_reel(api_base: &str, args: Value) -> Result<Value, String> {
    let base = require_api_base(api_base, "play_reel")?;
    let id = args.get("id").and_then(|v| v.as_str()).unwrap_or("");
    if id.is_empty() {
        return Err("play_reel: 'id' is required".to_string());
    }
    let control = json!({
        "action": "navigate_to",
        "params": {"kind": "reel", "id": id, "autoplay": true},
        "issuer": "mcp"
    });
    reqwest::Client::new()
        .post(format!("{base}/api/control"))
        .json(&control)
        .send()
        .await
        .map_err(|e| format!("play_reel: {e}"))?
        .json()
        .await
        .map_err(|e| format!("play_reel: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_reel_schema_requires_title_and_stops() {
        let s = save_reel_schema();
        let req: Vec<&str> = s["required"]
            .as_array().unwrap().iter().filter_map(|v| v.as_str()).collect();
        assert!(req.contains(&"title") && req.contains(&"stops"));
        // Rich beats R1: only `line` is schema-required per stop — spotlight's
        // sessionId/eventId anchor is enforced per-kind by the server on save.
        assert_eq!(s["properties"]["stops"]["items"]["required"],
            serde_json::json!(["line"]));
    }

    #[test]
    fn play_reel_schema_requires_id() {
        let s = play_reel_schema();
        assert_eq!(s["required"], serde_json::json!(["id"]));
    }
}
