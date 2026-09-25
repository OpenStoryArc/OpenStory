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
                // F-02: a mirror's or aggregate's sources with their cursors.
                "sources": s.get("sources").cloned().unwrap_or_else(|| json!([])),
            })
        })
        .collect();
    // The node's domain and the server's file store, so an aggregate's cap
    // can be read against what the server holds (F-01).
    Ok(json!({ "streams": streams, "jetstream": health["jetstream"] }))
}

/// `fleet_presence {}` — every node's latest beat with staleness (P-03).
pub async fn fleet_presence(api_base: &str, _args: Value) -> Result<Value, String> {
    let base = require_api_base(api_base, "fleet_presence")?;
    get_json(&format!("{base}/api/fleet/presence"), "fleet_presence").await
}

/// `consistency_report {}` — this node against every peer's beat (C-03),
/// in the verdict's shape: `diverged:<host>`, `behind:<host>`,
/// `lag:<consumer>`, `unverified`, `stale_snapshot:<host>`.
pub async fn consistency_report(api_base: &str, _args: Value) -> Result<Value, String> {
    let base = require_api_base(api_base, "consistency_report")?;
    get_json(&format!("{base}/api/consistency"), "consistency_report").await
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

/// What changed between two verdict-shaped bodies: the level moved, or a
/// finding appeared or cleared. `None` when nothing did, so a subscriber
/// hears only transitions. The new body rides under `key`. Pure.
pub fn transition(prev: &Value, next: &Value, key: &str) -> Option<Value> {
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
        key: next,
    }))
}

/// M-05: the health verdict's transition, under `verdict`.
pub fn health_transition(prev: &Value, next: &Value) -> Option<Value> {
    transition(prev, next, "verdict")
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

/// The consistency report (C-04), or an `unknown` report naming the error
/// when the node cannot be read, so an outage is itself a transition.
pub async fn read_report(api_base: &str) -> Value {
    match consistency_report(api_base, Value::Null).await {
        Ok(body) if body.get("level").is_some() => body,
        Ok(_) => json!({"level": "unknown", "findings": [
            {"id": "no_report", "level": "warn", "text": "the node serves no consistency report on this build"}]}),
        Err(e) => json!({"level": "unknown", "findings": [
            {"id": "consistency_unreachable", "level": "critical", "text": e}]}),
    }
}

// ── Tier 1 (M-06, M-07) ─────────────────────────────────────────────────────

fn tier_one_schema(required: &[&str], own: Value) -> Value {
    let mut props = serde_json::Map::new();
    if let Some(o) = own.as_object() {
        props.extend(o.iter().map(|(k, v)| (k.clone(), v.clone())));
    }
    props.insert("evidence".into(), json!({"type": "array", "items": {"type": "string"}, "description": "Finding ids from node_health that justify this"}));
    props.insert(
        "idempotency_key".into(),
        json!({"type": "string", "description": "Reuse to retry safely; generated when absent"}),
    );
    props.insert(
        "author".into(),
        json!({"type": "string", "description": "Who proposes (default mcp)"}),
    );
    json!({"type": "object", "properties": props, "required": required, "additionalProperties": false})
}

pub fn node_reproject_schema() -> Value {
    tier_one_schema(
        &[],
        json!({"session_id": {"type": "string", "description": "One session to rebuild; all when absent"}}),
    )
}
pub fn node_verify_schema() -> Value {
    tier_one_schema(&["session_id"], json!({"session_id": {"type": "string"}}))
}
pub fn node_catch_up_schema() -> Value {
    tier_one_schema(
        &[],
        json!({"peer": {"type": "string", "description": "Peer base URL; the node's configured peer when absent"}}),
    )
}
pub fn node_converge_schema() -> Value {
    tier_one_schema(
        &[],
        json!({
            "peers": {"type": "array", "items": {"type": "string"}, "description": "Peer base URLs; when absent, the configured catch-up peer, else every beat that advertises an api_url"},
            "max_rounds": {"type": "integer", "minimum": 1, "description": "Rounds before giving up (server default 3)"}
        }),
    )
}
pub fn node_prune_schema() -> Value {
    tier_one_schema(
        &["older_than_days"],
        json!({"older_than_days": {"type": "integer", "minimum": 1}}),
    )
}

/// A tier-1 hand: refuse up front unless the node is serving, leave the
/// proposal on the bus, then call the node and return what it did.
pub async fn tier_one<S: crate::subscription::Subscribe>(
    server: &crate::server::Server<S>,
    hand: &str,
    mut args: Value,
) -> Result<Value, String> {
    let tool = format!("node_{hand}");
    let base = require_api_base(&server.api_base, &tool)?;
    let health = get_json(&format!("{base}/api/health"), &tool).await?;
    let phase = health["boot"]["phase"].as_str().unwrap_or("unknown");
    if phase != "serving" {
        let r = &health["boot"]["replay"];
        let progress = match (r["done"].as_u64(), r["total"].as_u64()) {
            (Some(d), Some(t)) if t > 0 => format!(" ({d} of {t} sessions)"),
            _ => String::new(),
        };
        return Err(format!(
            "{tool}: node is {phase}{progress}; tier-1 hands wait for serving"
        ));
    }
    let obj = args
        .as_object_mut()
        .ok_or_else(|| format!("{tool}: arguments must be an object"))?;
    let key = match obj
        .remove("idempotency_key")
        .and_then(|v| v.as_str().map(str::to_string))
    {
        Some(k) if !k.is_empty() => k,
        _ => uuid::Uuid::new_v4().to_string(),
    };
    let author = obj
        .remove("author")
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "mcp".to_string());
    let evidence: Vec<String> = obj
        .remove("evidence")
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|s| s.as_str().map(str::to_string))
        .collect();
    let hand_args = Value::Object(obj.clone());

    // Propose first: no proposal, no act.
    let ce =
        open_story_core::ops::proposal_event(hand, &author, &evidence, &key, hand_args.clone());
    let batch = open_story_bus::IngestBatch {
        session_id: open_story_core::ops::ops_session_id(hand),
        project_id: open_story_core::ops::SOURCE.to_string(),
        events: vec![ce],
    };
    server
        .subscriber
        .publish_proposal(hand, &batch)
        .await
        .map_err(|e| format!("{tool}: proposal not published, not acting: {e:#}"))?;

    let mut body = hand_args;
    body["idempotency_key"] = json!(key);
    body["author"] = json!(author);
    body["evidence"] = json!(evidence);
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/ops/{hand}"))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("{tool}: {e}"))?;
    let status = resp.status();
    let answer: Value = resp.json().await.map_err(|e| format!("{tool}: {e}"))?;
    if !status.is_success() {
        let msg = answer["error"].as_str().unwrap_or("no detail");
        return Err(format!("{tool}: {status}: {msg}"));
    }
    Ok(json!({
        "hand": hand,
        "proposal": {
            "subject": open_story_core::ops::proposal_subject(hand),
            "idempotency_key": key,
            "author": author,
            "evidence": evidence,
        },
        "replayed": answer["replayed"],
        "ok": answer["ok"],
        "result": answer["result"],
        "command_subject": answer["command_subject"],
    }))
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
