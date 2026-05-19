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
//! The transport is generic over `S: Subscribe`. Production passes
//! `NatsBus`. Integration tests pass a `LoopbackSubscriber`.

use crate::subscription::Subscribe;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Mutex};

/// Drive a stdio MCP session to completion. Returns when stdin closes.
pub async fn run<R, W, S>(input: R, output: W, subscriber: S) -> Result<()>
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
        handle_line(&line, &subscriber, &tx, &subs).await;
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
    subscriber: &S,
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

    // tools/call subscribe_session / subscribe_tokens — start a stream.
    if method == "tools/call" {
        let name = parsed
            .get("params")
            .and_then(|p| p.get("name"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        match name {
            "subscribe_session" => {
                handle_subscribe_session(parsed, subscriber, out, subs).await;
                return;
            }
            "subscribe_tokens" => {
                handle_subscribe_tokens(parsed, subscriber, out, subs).await;
                return;
            }
            _ => {}
        }
    }

    // Everything else: delegate to the pure protocol handler.
    if let Some(resp) = crate::protocol::handle_message(line) {
        let _ = out.send(serde_json::to_string(&resp).unwrap()).await;
    }
}

async fn handle_subscribe_session<S: Subscribe>(
    parsed: Value,
    subscriber: &S,
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

    let mut subscription = match subscriber.subscribe(&session_id).await {
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
            if pump_out.send(serde_json::to_string(&notif).unwrap()).await.is_err() {
                break;
            }
        }
    });

    subs.lock().await.insert(id_key, handle);
}

async fn handle_subscribe_tokens<S: Subscribe>(
    parsed: Value,
    subscriber: &S,
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

    let mut subscription = match subscriber.subscribe(&session_id).await {
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
                if pump_out.send(serde_json::to_string(&notif).unwrap()).await.is_err() {
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
