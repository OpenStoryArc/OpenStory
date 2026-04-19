#!/usr/bin/env node
/**
 * api-proxy.mjs — Transparent logging proxy for Anthropic API research.
 *
 * Sits between pi-mono and api.anthropic.com. Logs every request body
 * and SSE response event to a JSONL file for offline analysis.
 *
 * Usage:
 *   node scripts/research/api-proxy.mjs [--port 9090] [--log /tmp/api-trace.jsonl]
 *
 * Then run pi-mono with:
 *   ANTHROPIC_BASE_URL=http://localhost:9090 pi "hello"
 *
 * Or set HTTP_PROXY=http://localhost:9090 if using undici proxy support.
 */

import { createServer } from "node:http";
import { appendFileSync, writeFileSync } from "node:fs";
import { URL } from "node:url";

const PORT = parseInt(process.argv.find((_, i, a) => a[i - 1] === "--port") || "9090");
const LOG_FILE = process.argv.find((_, i, a) => a[i - 1] === "--log") || "/tmp/api-trace.jsonl";
const UPSTREAM = "https://api.anthropic.com";

let requestSeq = 0;

function log(entry) {
  const line = JSON.stringify({ ...entry, ts: new Date().toISOString() });
  appendFileSync(LOG_FILE, line + "\n");
}

writeFileSync(LOG_FILE, ""); // clear on start
console.log(`API proxy listening on http://localhost:${PORT}`);
console.log(`Forwarding to ${UPSTREAM}`);
console.log(`Logging to ${LOG_FILE}`);
console.log();

const server = createServer(async (req, res) => {
  const seq = ++requestSeq;
  const targetUrl = `${UPSTREAM}${req.url}`;

  // Collect request body
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  const bodyBuf = Buffer.concat(chunks);
  const bodyStr = bodyBuf.toString("utf-8");

  // Log the request
  let requestBody;
  try {
    requestBody = JSON.parse(bodyStr);
  } catch {
    requestBody = bodyStr;
  }

  log({
    type: "request",
    seq,
    method: req.method,
    url: req.url,
    model: requestBody?.model,
    message_count: requestBody?.messages?.length,
    tool_count: requestBody?.tools?.length,
    stream: requestBody?.stream,
    has_thinking: !!requestBody?.thinking,
    // Don't log full messages/system prompt — too large. Log shapes.
    message_roles: requestBody?.messages?.map((m) => m.role),
    content_block_types: requestBody?.messages?.map((m) =>
      Array.isArray(m.content) ? m.content.map((b) => b.type) : typeof m.content
    ),
  });

  // Forward headers (strip host, add correct one)
  const forwardHeaders = { ...Object.fromEntries(
    Object.entries(req.headers).filter(([k]) => k !== "host")
  )};

  try {
    const upstream = await fetch(targetUrl, {
      method: req.method,
      headers: forwardHeaders,
      body: req.method !== "GET" && req.method !== "HEAD" ? bodyBuf : undefined,
      // Don't decompress — we want raw SSE
    });

    // Forward status + headers
    const responseHeaders = {};
    upstream.headers.forEach((v, k) => {
      if (k !== "transfer-encoding" && k !== "content-encoding") {
        responseHeaders[k] = v;
      }
    });
    res.writeHead(upstream.status, responseHeaders);

    const isSSE = upstream.headers.get("content-type")?.includes("text/event-stream");

    if (isSSE && upstream.body) {
      // Stream SSE events through, logging each one
      const reader = upstream.body.getReader();
      const decoder = new TextDecoder();
      let buffer = "";
      let eventCount = 0;

      while (true) {
        const { done, value } = await reader.read();
        if (done) break;

        const text = decoder.decode(value, { stream: true });
        res.write(text); // forward immediately
        buffer += text;

        // Parse SSE events from buffer
        const parts = buffer.split("\n\n");
        buffer = parts.pop(); // keep incomplete part

        for (const part of parts) {
          if (!part.trim()) continue;
          const lines = part.split("\n");
          let eventType = null;
          let data = null;

          for (const line of lines) {
            if (line.startsWith("event: ")) eventType = line.slice(7);
            if (line.startsWith("data: ")) data = line.slice(6);
          }

          if (eventType && data) {
            eventCount++;
            try {
              const parsed = JSON.parse(data);
              // Log a compact version of each SSE event
              const entry = { type: "sse", seq, event: eventType, n: eventCount };

              if (eventType === "message_start") {
                entry.usage = parsed.message?.usage;
              } else if (eventType === "content_block_start") {
                entry.index = parsed.index;
                entry.block_type = parsed.content_block?.type;
                if (parsed.content_block?.type === "tool_use") {
                  entry.tool_name = parsed.content_block.name;
                  entry.tool_id = parsed.content_block.id;
                }
              } else if (eventType === "content_block_delta") {
                entry.index = parsed.index;
                entry.delta_type = parsed.delta?.type;
                entry.delta_length = (
                  parsed.delta?.text ||
                  parsed.delta?.thinking ||
                  parsed.delta?.partial_json ||
                  ""
                ).length;
              } else if (eventType === "content_block_stop") {
                entry.index = parsed.index;
              } else if (eventType === "message_delta") {
                entry.stop_reason = parsed.delta?.stop_reason;
                entry.usage = parsed.usage;
              }

              log(entry);
            } catch {
              log({ type: "sse_raw", seq, event: eventType, data });
            }
          }
        }
      }

      log({ type: "response_end", seq, event_count: eventCount });
      res.end();
    } else {
      // Non-streaming response — forward as-is
      const body = await upstream.arrayBuffer();
      log({ type: "response", seq, status: upstream.status, bytes: body.byteLength });
      res.end(Buffer.from(body));
    }
  } catch (err) {
    log({ type: "error", seq, error: err.message });
    res.writeHead(502);
    res.end(`Proxy error: ${err.message}`);
  }
});

server.listen(PORT);
