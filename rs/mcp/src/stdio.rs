//! Stdio transport for the MCP server.
//!
//! Reads line-delimited JSON-RPC messages from `stdin`, dispatches them,
//! and writes responses (one per line) to `stdout`. Notifications produce
//! no output.
//!
//! Streaming tools (`subscribe_session`, `subscribe_tokens`) are
//! special-cased: the tool call returns immediately with
//! `{stream_id, status: "started"}`, then a background task pumps events
//! from the bus to stdout as notification lines tagged with the stream_id.
//! Client cancels via `notifications/cancelled` referencing the original
//! request id.
//!
//! The transport is generic over `S: Subscribe`. Production passes a
//! `Server { subscriber: NatsBus, store: SqliteStore }`. Integration
//! tests pass a `Server { subscriber: LoopbackSubscriber, store: <temp> }`.

use crate::server::Server;
use crate::subscription::Subscribe;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Mutex};

/// Drive a stdio MCP session to completion. Returns when stdin closes.
pub async fn run<R, W, S>(input: R, output: W, server: Server<S>) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
    S: Subscribe,
{
    // Single writer task — every line that needs to leave the process
    // goes through this mpsc so the wire stays free of interleaved bytes.
    let (tx, mut rx) = mpsc::channel::<String>(1024);
    let writer = tokio::spawn(async move {
        let mut output = output;
        while let Some(mut line) = rx.recv().await {
            if !line.ends_with('\n') {
                line.push('\n');
            }
            if output.write_all(line.as_bytes()).await.is_err() {
                break;
            }
            let _ = output.flush().await;
        }
    });

    // Active subscriptions keyed by the request id that opened them, so
    // notifications/cancelled (which references the request id) can find
    // and tear them down.
    let subs: Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    let mut reader = BufReader::new(input).lines();
    while let Some(line) = reader.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        handle_line(&line, &server, &tx, &subs).await;
    }

    // Stdin closed — tear down everything.
    {
        let mut subs = subs.lock().await;
        for (_, handle) in subs.drain() {
            handle.abort();
        }
    }
    drop(tx);
    let _ = writer.await;
    Ok(())
}

async fn handle_line<S: Subscribe>(
    line: &str,
    server: &Server<S>,
    out: &mpsc::Sender<String>,
    subs: &Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
) {
    let parsed: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(_) => {
            let resp = crate::protocol::JsonRpcResponse::parse_error();
            let _ = out.send(serde_json::to_string(&resp).unwrap()).await;
            return;
        }
    };

    let method = parsed.get("method").and_then(|v| v.as_str()).unwrap_or("");

    // notifications/cancelled — tear down the matching subscription.
    if method == "notifications/cancelled" {
        if let Some(cancel_id) = parsed
            .get("params")
            .and_then(|p| p.get("requestId"))
            .map(id_as_key)
        {
            if let Some(handle) = subs.lock().await.remove(&cancel_id) {
                handle.abort();
            }
        }
        return;
    }

    // tools/call routing:
    //   subscribe_* → streaming handlers (need writer + JSON-RPC id)
    //   everything else → async query dispatch (needs store)
    if method == "tools/call" {
        let id = parsed.get("id").cloned().unwrap_or(Value::Null);
        let name = parsed
            .get("params")
            .and_then(|p| p.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let args = parsed
            .get("params")
            .and_then(|p| p.get("arguments"))
            .cloned()
            .unwrap_or(Value::Null);

        match name {
            "subscribe_session" => {
                handle_subscribe_session(parsed, server, out, subs).await;
            }
            "subscribe_tokens" => {
                handle_subscribe_tokens(parsed, server, out, subs).await;
            }
            "subscribe_ui_state" => {
                handle_subscribe_ui_state(parsed, server, out, subs).await;
            }
            "subscribe_health" => {
                handle_subscribe_pump(parsed, server, out, subs, &HEALTH_PUMP).await;
            }
            "subscribe_convergence" => {
                handle_subscribe_pump(parsed, server, out, subs, &CONVERGENCE_PUMP).await;
            }
            _ => {
                let result = crate::tools::dispatch_query_tool(server, name, args).await;
                let response = crate::protocol::JsonRpcResponse::success(id, result);
                let _ = out.send(serde_json::to_string(&response).unwrap()).await;
            }
        }
        return;
    }

    // Everything else (initialize, tools/list, …): pure protocol handler.
    if let Some(resp) = crate::protocol::handle_message(line) {
        let _ = out.send(serde_json::to_string(&resp).unwrap()).await;
    }
}

async fn handle_subscribe_session<S: Subscribe>(
    parsed: Value,
    server: &Server<S>,
    out: &mpsc::Sender<String>,
    subs: &Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
) {
    let id = parsed.get("id").cloned().unwrap_or(Value::Null);
    let id_key = id_as_key(&id);
    let session_id = parsed
        .get("params")
        .and_then(|p| p.get("arguments"))
        .and_then(|a| a.get("session_id"))
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let Some(session_id) = session_id else {
        let resp = crate::protocol::JsonRpcResponse::failure(
            id,
            crate::protocol::error_code::INVALID_PARAMS,
            "subscribe_session requires `session_id`",
        );
        let _ = out.send(serde_json::to_string(&resp).unwrap()).await;
        return;
    };

    let mut subscription = match server.subscriber.subscribe(&session_id).await {
        Ok(s) => s,
        Err(e) => {
            let resp = crate::protocol::JsonRpcResponse::failure(
                id,
                crate::protocol::error_code::INTERNAL_ERROR,
                &format!("subscribe failed: {e}"),
            );
            let _ = out.send(serde_json::to_string(&resp).unwrap()).await;
            return;
        }
    };
    let stream_id = subscription.stream_id.to_string();

    let result = json!({
        "isError": false,
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({
                "stream_id": stream_id,
                "session_id": session_id,
                "status": "started",
            })).unwrap(),
        }]
    });
    let response = crate::protocol::JsonRpcResponse::success(id, result);
    let _ = out.send(serde_json::to_string(&response).unwrap()).await;

    let pump_out = out.clone();
    let pump_stream_id = stream_id.clone();
    let handle = tokio::spawn(async move {
        while let Some(event) = subscription.recv().await {
            let notif = json!({
                "jsonrpc": "2.0",
                "method": "notifications/openstory/stream",
                "params": {
                    "stream_id": pump_stream_id,
                    "seq": event.seq,
                    "session_id": event.session_id,
                    "data": event.data,
                }
            });
            if pump_out
                .send(serde_json::to_string(&notif).unwrap())
                .await
                .is_err()
            {
                break;
            }
        }
    });

    subs.lock().await.insert(id_key, handle);
}

/// `subscribe_ui_state` — live-follow WHERE THE USER IS (the READ half of the
/// agent-in-UI seam). Subscribes to the authored `ui.*` stream and emits a
/// JSON-RPC notification per interaction, shaped by `ui_state_notification`
/// (same summary as `where_is_user`). No args. Pair with `ui_control` to drive
/// from where the user just moved.
async fn handle_subscribe_ui_state<S: Subscribe>(
    parsed: Value,
    server: &Server<S>,
    out: &mpsc::Sender<String>,
    subs: &Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
) {
    let id = parsed.get("id").cloned().unwrap_or(Value::Null);
    let id_key = id_as_key(&id);

    let mut subscription = match server.subscriber.subscribe_ui().await {
        Ok(s) => s,
        Err(e) => {
            let resp = crate::protocol::JsonRpcResponse::failure(
                id,
                crate::protocol::error_code::INTERNAL_ERROR,
                &format!("subscribe_ui_state failed: {e}"),
            );
            let _ = out.send(serde_json::to_string(&resp).unwrap()).await;
            return;
        }
    };
    let stream_id = subscription.stream_id.to_string();

    let result = json!({
        "isError": false,
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({
                "stream_id": stream_id,
                "status": "started",
                "following": "ui-state — where the user is, live",
            })).unwrap(),
        }]
    });
    let response = crate::protocol::JsonRpcResponse::success(id, result);
    let _ = out.send(serde_json::to_string(&response).unwrap()).await;

    let pump_out = out.clone();
    let pump_stream_id = stream_id.clone();
    let handle = tokio::spawn(async move {
        while let Some(event) = subscription.recv().await {
            let notif = json!({
                "jsonrpc": "2.0",
                "method": "notifications/openstory/ui_state",
                "params": {
                    "stream_id": pump_stream_id,
                    "seq": event.seq,
                    "ui_state": crate::tools::control::ui_state_notification(&event.data),
                }
            });
            if pump_out
                .send(serde_json::to_string(&notif).unwrap())
                .await
                .is_err()
            {
                break;
            }
        }
    });

    subs.lock().await.insert(id_key, handle);
}

/// A verdict-shaped body polled on an interval and spoken only on
/// transitions (M-05, C-04): which hand, what it reads, what it says.
struct Pump {
    tool: &'static str,
    /// The key the body rides under in the ack and in each notification.
    key: &'static str,
    method: &'static str,
    following: &'static str,
    read: fn(&str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Value> + Send>>,
}

fn read_verdict(base: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Value> + Send>> {
    let base = base.to_string();
    Box::pin(async move { crate::tools::ops::read_verdict(&base).await })
}

fn read_report(base: &str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Value> + Send>> {
    let base = base.to_string();
    Box::pin(async move { crate::tools::ops::read_report(&base).await })
}

static HEALTH_PUMP: Pump = Pump {
    tool: "subscribe_health",
    key: "verdict",
    method: "notifications/openstory/health",
    following: "health — verdict transitions and findings added or cleared",
    read: read_verdict,
};

static CONVERGENCE_PUMP: Pump = Pump {
    tool: "subscribe_convergence",
    key: "report",
    method: "notifications/openstory/convergence",
    following:
        "convergence — the consistency report's level and findings, spoken only on transitions",
    read: read_report,
};

/// Poll the pump's body and tell the client only when it moves.
async fn handle_subscribe_pump<S: Subscribe>(
    parsed: Value,
    server: &Server<S>,
    out: &mpsc::Sender<String>,
    subs: &Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
    pump: &'static Pump,
) {
    let id = parsed.get("id").cloned().unwrap_or(Value::Null);
    let id_key = id_as_key(&id);
    let api_base = server.api_base.trim_end_matches('/').to_string();
    if api_base.trim().is_empty() {
        let resp = crate::protocol::JsonRpcResponse::failure(
            id,
            crate::protocol::error_code::INTERNAL_ERROR,
            &format!(
                "{} unavailable: the MCP has no API base configured (set OPENSTORY_API_URL)",
                pump.tool
            ),
        );
        let _ = out.send(serde_json::to_string(&resp).unwrap()).await;
        return;
    }
    let interval_secs = parsed
        .get("params")
        .and_then(|p| p.get("arguments"))
        .and_then(|a| a.get("interval_secs"))
        .and_then(|v| v.as_f64())
        .filter(|s| *s > 0.0)
        .unwrap_or(15.0);
    let interval = std::time::Duration::from_secs_f64(interval_secs);
    let stream_id = uuid::Uuid::new_v4().to_string();

    let mut last = (pump.read)(&api_base).await;
    let result = json!({
        "isError": false,
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({
                "stream_id": stream_id,
                "status": "started",
                "following": pump.following,
                "interval_secs": interval_secs,
                pump.key: last,
            })).unwrap(),
        }]
    });
    let response = crate::protocol::JsonRpcResponse::success(id, result);
    let _ = out.send(serde_json::to_string(&response).unwrap()).await;

    let pump_out = out.clone();
    let pump_stream_id = stream_id.clone();
    let handle = tokio::spawn(async move {
        let mut seq: u64 = 0;
        let mut tick = tokio::time::interval(interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        tick.tick().await; // the first tick is immediate; the ack already read once
        loop {
            tick.tick().await;
            let next = (pump.read)(&api_base).await;
            if let Some(mut change) = crate::tools::ops::transition(&last, &next, pump.key) {
                seq += 1;
                change["stream_id"] = json!(pump_stream_id);
                change["seq"] = json!(seq);
                let notif = json!({
                    "jsonrpc": "2.0",
                    "method": pump.method,
                    "params": change,
                });
                if pump_out
                    .send(serde_json::to_string(&notif).unwrap())
                    .await
                    .is_err()
                {
                    break;
                }
            }
            last = next;
        }
    });

    subs.lock().await.insert(id_key, handle);
}

async fn handle_subscribe_tokens<S: Subscribe>(
    parsed: Value,
    server: &Server<S>,
    out: &mpsc::Sender<String>,
    subs: &Arc<Mutex<HashMap<String, tokio::task::JoinHandle<()>>>>,
) {
    let id = parsed.get("id").cloned().unwrap_or(Value::Null);
    let id_key = id_as_key(&id);
    let session_id = parsed
        .get("params")
        .and_then(|p| p.get("arguments"))
        .and_then(|a| a.get("session_id"))
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let Some(session_id) = session_id else {
        let resp = crate::protocol::JsonRpcResponse::failure(
            id,
            crate::protocol::error_code::INVALID_PARAMS,
            "subscribe_tokens requires `session_id`",
        );
        let _ = out.send(serde_json::to_string(&resp).unwrap()).await;
        return;
    };

    let mut subscription = match server.subscriber.subscribe(&session_id).await {
        Ok(s) => s,
        Err(e) => {
            let resp = crate::protocol::JsonRpcResponse::failure(
                id,
                crate::protocol::error_code::INTERNAL_ERROR,
                &format!("subscribe failed: {e}"),
            );
            let _ = out.send(serde_json::to_string(&resp).unwrap()).await;
            return;
        }
    };
    let stream_id = subscription.stream_id.to_string();

    let result = json!({
        "isError": false,
        "content": [{
            "type": "text",
            "text": serde_json::to_string(&json!({
                "stream_id": stream_id,
                "session_id": session_id,
                "status": "started",
                "watching": "tokens"
            })).unwrap(),
        }]
    });
    let response = crate::protocol::JsonRpcResponse::success(id, result);
    let _ = out.send(serde_json::to_string(&response).unwrap()).await;

    let pump_out = out.clone();
    let pump_stream_id = stream_id.clone();
    let handle = tokio::spawn(async move {
        let mut agg = crate::tokens::TokenAggregator::new();
        let mut seq: u64 = 1;
        while let Some(event) = subscription.recv().await {
            // event.data is the IngestBatch JSON shape:
            //   { session_id, project_id, events: [CloudEvent, ...] }
            // TokenAggregator walks events[*].data.raw.message.usage.
            if let Some((delta, running)) = agg.observe(&event.data) {
                let notif = json!({
                    "jsonrpc": "2.0",
                    "method": "notifications/openstory/tokens",
                    "params": {
                        "stream_id": pump_stream_id,
                        "seq": seq,
                        "session_id": event.session_id,
                        "delta": delta,
                        "running": running,
                        "total": running.total(),
                    }
                });
                seq += 1;
                if pump_out
                    .send(serde_json::to_string(&notif).unwrap())
                    .await
                    .is_err()
                {
                    break;
                }
            }
        }
    });

    subs.lock().await.insert(id_key, handle);
}

fn id_as_key(id: &Value) -> String {
    match id {
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}
