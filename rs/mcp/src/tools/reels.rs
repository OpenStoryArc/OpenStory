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
                "description": "Ordered stops — each spotlights ONE real recorded event with one narration line",
                "items": {
                    "type": "object",
                    "properties": {
                        "sessionId": {"type": "string"},
                        "eventId": {"type": "string"},
                        "line": {"type": "string", "description": "Narration/caption text, written for the ear"},
                        "clipAt": {"type": "string", "description": "Optional camera crop: show text before this marker"}
                    },
                    "required": ["sessionId", "eventId", "line"],
                    "additionalProperties": false
                }
            },
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

pub async fn save_reel(api_base: &str, mut args: Value) -> Result<Value, String> {
    if args.get("author").and_then(|a| a.as_str()).unwrap_or("").is_empty() {
        args["author"] = json!("mcp");
    }
    let resp = reqwest::Client::new()
        .post(format!("{api_base}/api/reels"))
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
    reqwest::Client::new()
        .get(format!("{api_base}/api/reels"))
        .send()
        .await
        .map_err(|e| format!("list_reels: {e}"))?
        .json()
        .await
        .map_err(|e| format!("list_reels: {e}"))
}

pub async fn play_reel(api_base: &str, args: Value) -> Result<Value, String> {
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
        .post(format!("{api_base}/api/control"))
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
        assert_eq!(s["properties"]["stops"]["items"]["required"],
            serde_json::json!(["sessionId", "eventId", "line"]));
    }

    #[test]
    fn play_reel_schema_requires_id() {
        let s = play_reel_schema();
        assert_eq!(s["required"], serde_json::json!(["id"]));
    }
}
