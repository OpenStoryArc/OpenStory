//! Ops hands, tier 0 (REQUIREMENTS group M): watch and diagnose a node.
//!
//! Every hand here reads the REST API and writes nothing. The verdict comes
//! from the node itself (`/api/health` carries it since M-01), so an agent,
//! the header dot, and the probe all read one answer. Motions: **watch**
//! (fleet_presence, subscribe_health) and **diagnose** (node_health,
//! node_logs, node_streams).

use serde_json::{json, Value};

/// Same guard as the reel and control hands: a blank base is a clear error
/// naming the env var; a trailing slash is trimmed.
fn require_api_base(api_base: &str, tool: &str) -> Result<String, String> {
    if api_base.trim().is_empty() {
        return Err(format!(
            "{tool} unavailable: the MCP has no API base configured (set OPENSTORY_API_URL)"
        ));
    }
    Ok(api_base.trim_end_matches('/').to_string())
}

/// GET a JSON body. A 503 still carries a body (health while replaying),
/// so the status is not an error here; a transport failure is.
async fn get_json(url: &str, tool: &str) -> Result<Value, String> {
    reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|e| format!("{tool}: {e}"))?
        .json()
        .await
        .map_err(|e| format!("{tool}: {e}"))
}

pub fn empty_schema() -> Value {
    json!({"type": "object", "properties": {}, "additionalProperties": false})
}

pub fn node_logs_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "since": {"type": "integer", "description": "Only lines with seq greater than this (from a previous `next`)"},
            "actor": {"type": "string", "description": "Only this actor's lines (persist, patterns, presence, claude-code, …)"},
            "level": {"type": "string", "description": "Minimum level: TRACE, DEBUG, INFO, WARN, ERROR"},
            "limit": {"type": "integer", "description": "At most this many lines (server default 200)"}
        },
        "additionalProperties": false
    })
}

/// `node_health {}` — the health body with the node's own verdict.
pub async fn node_health(api_base: &str, _args: Value) -> Result<Value, String> {
    let base = require_api_base(api_base, "node_health")?;
    get_json(&format!("{base}/api/health"), "node_health").await
}

/// The query string for `/api/logs` from the hand's arguments: only the
/// filters given, in a fixed order.
pub fn logs_query(args: &Value) -> String {
    let mut parts = Vec::new();
    for key in ["since", "actor", "level", "limit"] {
        match args.get(key) {
            Some(Value::String(s)) if !s.is_empty() => parts.push(format!("{key}={s}")),
            Some(Value::Number(n)) => parts.push(format!("{key}={n}")),
            _ => {}
        }
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("?{}", parts.join("&"))
    }
}

/// `node_logs {since?, actor?, level?, limit?}` — the log ring (L-06).
pub async fn node_logs(api_base: &str, args: Value) -> Result<Value, String> {
    let base = require_api_base(api_base, "node_logs")?;
    get_json(
        &format!("{base}/api/logs{}", logs_query(&args)),
        "node_logs",
    )
    .await
}

/// The verdict's stream thresholds, so a stream's level here matches the
/// finding the node would raise.
pub fn stream_level(percent: Option<f64>) -> &'static str {
    match percent {
        Some(p) if p >= 0.90 => "critical",
        Some(p) if p >= 0.70 => "warn",
        _ => "ok",
    }
}

/// `node_streams {}` — each stream's bytes against its cap, with a level.
pub async fn node_streams(api_base: &str, _args: Value) -> Result<Value, String> {
    let base = require_api_base(api_base, "node_streams")?;
    let health = get_json(&format!("{base}/api/health"), "node_streams").await?;
    let streams: Vec<Value> = health["streams"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| {
            let percent = s["percent"].as_f64();
            json!({
                "name": s["name"],
                "bytes": s["bytes"],
                "messages": s["messages"],
                "max_bytes": s["max_bytes"],
                "percent": percent,
                "level": stream_level(percent),
            })
        })
        .collect();
    Ok(json!({ "streams": streams }))
}

/// `fleet_presence {}` — every node's latest beat with staleness (P-03).
pub async fn fleet_presence(api_base: &str, _args: Value) -> Result<Value, String> {
    let base = require_api_base(api_base, "fleet_presence")?;
    get_json(&format!("{base}/api/fleet/presence"), "fleet_presence").await
}

pub fn subscribe_health_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "interval_secs": {"type": "number", "description": "Seconds between health reads (default 15)"}
        },
        "additionalProperties": false
    })
}

fn finding_ids(verdict: &Value) -> Vec<String> {
    let mut ids: Vec<String> = verdict["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f["id"].as_str().map(str::to_string))
        .collect();
    ids.sort();
    ids
}

/// What changed between two verdicts (M-05): the level moved, or a
/// finding appeared or cleared. `None` when nothing did, so a subscriber
/// hears only transitions. Pure.
pub fn health_transition(prev: &Value, next: &Value) -> Option<Value> {
    let from = prev["level"].as_str().unwrap_or("unknown");
    let to = next["level"].as_str().unwrap_or("unknown");
    let before = finding_ids(prev);
    let after = finding_ids(next);
    let added: Vec<&String> = after.iter().filter(|id| !before.contains(id)).collect();
    let cleared: Vec<&String> = before.iter().filter(|id| !after.contains(id)).collect();
    if from == to && added.is_empty() && cleared.is_empty() {
        return None;
    }
    Some(json!({
        "from": from,
        "to": to,
        "added": added,
        "cleared": cleared,
        "verdict": next,
    }))
}

/// The verdict on a health body, or an `unknown` verdict naming the error
/// when the node cannot be read, so an outage is itself a transition.
pub async fn read_verdict(api_base: &str) -> Value {
    match node_health(api_base, Value::Null).await {
        Ok(body) if body.get("verdict").is_some() => body["verdict"].clone(),
        Ok(_) => json!({"level": "unknown", "findings": [
            {"id": "no_verdict", "level": "warn", "text": "health has no verdict on this build"}]}),
        Err(e) => json!({"level": "unknown", "findings": [
            {"id": "health_unreachable", "level": "critical", "text": e}]}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logs_query_keeps_only_given_filters_in_order() {
        assert_eq!(logs_query(&json!({})), "");
        assert_eq!(logs_query(&json!({"actor": "presence"})), "?actor=presence");
        assert_eq!(
            logs_query(&json!({"limit": 5, "since": 3, "level": "WARN", "actor": ""})),
            "?since=3&level=WARN&limit=5"
        );
    }

    #[test]
    fn stream_levels_match_the_verdict() {
        assert_eq!(stream_level(None), "ok");
        assert_eq!(stream_level(Some(0.69)), "ok");
        assert_eq!(stream_level(Some(0.70)), "warn");
        assert_eq!(stream_level(Some(0.90)), "critical");
    }
}
