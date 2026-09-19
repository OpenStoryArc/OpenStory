//! Memory hands — write side (requirement D-04): `enrich`,
//! `adjudicate_boundary`, `link_saga`, `propose_keep`.
//!
//! Each hand POSTs a MemoryWrite to `{api_base}/api/memory`, the same REST
//! seam `ui_control` uses. The server validates (schema, author, and the
//! reading laws), stores, publishes on `memory.>`, and returns the stored
//! record, which the hand returns to the host. `HttpEventStore` stays
//! read-only by construction: there is no write path through the store.
//!
//! Input schemas are strict: a malformed judgment is refused at the
//! protocol before any request is made.

use serde_json::{json, Value};

fn author_schema() -> Value {
    json!({
        "type": "object",
        "description": "Who read: the host running the prompt and the model it used",
        "properties": { "host": {"type": "string"}, "model": {"type": "string"} },
        "required": ["host", "model"],
        "additionalProperties": false
    })
}

fn base_props() -> serde_json::Map<String, Value> {
    let mut m = serde_json::Map::new();
    m.insert(
        "handle".into(),
        json!({"type": "string", "description": "Arc handle (16 hex)"}),
    );
    m.insert(
        "session_id".into(),
        json!({"type": "string", "description": "Session the arc lives in"}),
    );
    m.insert("author".into(), author_schema());
    m
}

fn strict(mut props: serde_json::Map<String, Value>, extra: Value, required: &[&str]) -> Value {
    if let Some(obj) = extra.as_object() {
        for (k, v) in obj {
            props.insert(k.clone(), v.clone());
        }
    }
    json!({ "type": "object", "properties": props, "required": required, "additionalProperties": false })
}

pub fn enrich_schema() -> Value {
    strict(
        base_props(),
        json!({ "enrichment": {
            "type": "object",
            "description": "The narrate_arc output: title, question, resolution, summary, slots (see openstory://schemas/enrichment)",
            "properties": {
                "title": {"type": "string"}, "question": {"type": "string"}, "resolution": {"type": "string"}, "summary": {"type": "string"},
                "slots": {"type": "object", "properties": {
                    "decisions": {"type": "array", "items": {"type": "string"}}, "deferrals": {"type": "array", "items": {"type": "string"}},
                    "tradeoffs": {"type": "array", "items": {"type": "string"}}, "failures": {"type": "array", "items": {"type": "string"}},
                    "stance": {"type": "array", "items": {"type": "string"}} }, "additionalProperties": false }
            },
            "required": ["title", "question", "resolution", "summary"],
            "additionalProperties": false
        } }),
        &["handle", "session_id", "author", "enrichment"],
    )
}

pub fn adjudicate_schema() -> Value {
    strict(
        base_props(),
        json!({
            "seam": {"type": "integer", "minimum": 0, "description": "Exchange index the seam sits before"},
            "verdict": {"type": "string", "enum": ["same_theme", "new_theme"]},
            "reason": {"type": "string"}
        }),
        &[
            "handle",
            "session_id",
            "author",
            "seam",
            "verdict",
            "reason",
        ],
    )
}

pub fn link_saga_schema() -> Value {
    strict(
        base_props(),
        json!({
            "handles": {"type": "array", "items": {"type": "string"}, "minItems": 2, "description": "Arc handles the saga links, this one included"},
            "reason": {"type": "string"}
        }),
        &["handle", "session_id", "author", "handles", "reason"],
    )
}

pub fn propose_keep_schema() -> Value {
    strict(
        base_props(),
        json!({ "reason": {"type": "string"} }),
        &["handle", "session_id", "author", "reason"],
    )
}

fn field<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("requires `{key}`"))
}

/// Pure: the MemoryWrite body for one hand. Author is required up front.
fn write_body(kind: &str, args: &Value, payload: Value) -> Result<Value, String> {
    let handle = field(args, "handle")?;
    let session_id = field(args, "session_id")?;
    let author = args
        .get("author")
        .cloned()
        .ok_or("requires `author` { host, model }")?;
    if author
        .get("host")
        .and_then(|v| v.as_str())
        .map(str::is_empty)
        .unwrap_or(true)
        || author
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::is_empty)
            .unwrap_or(true)
    {
        return Err("author.host and author.model are required".into());
    }
    let mut payload = payload;
    payload["handle"] = json!(handle);
    payload["author"] = author.clone();
    Ok(
        json!({ "kind": kind, "handle": handle, "session_id": session_id, "author": author, "payload": payload }),
    )
}

/// Effect: POST the write to the server and return the stored record.
async fn post_memory(api_base: &str, body: Value) -> Result<Value, String> {
    if api_base.trim().is_empty() {
        return Err("write hands need OPENSTORY_API_URL (api_base) to reach /api/memory".into());
    }
    let url = format!("{}/api/memory", api_base.trim_end_matches('/'));
    let resp = reqwest::Client::new()
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("POST {url} failed: {e}"))?;
    let status = resp.status();
    let value: Value = resp
        .json()
        .await
        .map_err(|e| format!("POST {url}: bad JSON: {e}"))?;
    if !status.is_success() {
        let msg = value
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("rejected");
        return Err(format!("/api/memory {status}: {msg}"));
    }
    Ok(value)
}

pub async fn enrich(api_base: &str, args: Value) -> Result<Value, String> {
    let enrichment = args
        .get("enrichment")
        .cloned()
        .ok_or("requires `enrichment`")?;
    post_memory(api_base, write_body("enrichment", &args, enrichment)?).await
}

pub async fn adjudicate_boundary(api_base: &str, args: Value) -> Result<Value, String> {
    let seam = args
        .get("seam")
        .and_then(|v| v.as_u64())
        .ok_or("requires `seam`")?;
    let verdict = field(&args, "verdict")?;
    let reason = field(&args, "reason")?;
    let payload = json!({ "seam": seam, "verdict": verdict, "reason": reason });
    post_memory(api_base, write_body("verdict", &args, payload)?).await
}

pub async fn link_saga(api_base: &str, args: Value) -> Result<Value, String> {
    let handles = args.get("handles").cloned().ok_or("requires `handles`")?;
    let reason = field(&args, "reason")?;
    post_memory(
        api_base,
        write_body(
            "saga",
            &args,
            json!({ "handles": handles, "reason": reason }),
        )?,
    )
    .await
}

pub async fn propose_keep(api_base: &str, args: Value) -> Result<Value, String> {
    let reason = field(&args, "reason")?;
    post_memory(
        api_base,
        write_body("keep", &args, json!({ "reason": reason }))?,
    )
    .await
}
