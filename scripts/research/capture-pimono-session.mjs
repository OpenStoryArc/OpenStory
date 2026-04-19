#!/usr/bin/env node
/**
 * capture-pimono-session.mjs — Run a pi-mono session and capture everything.
 *
 * Creates an agent session via pi-mono's SDK, sends a controlled prompt,
 * and writes three outputs:
 *
 *   1. The JSONL session file (pi-mono's native persistence)
 *   2. A streaming events log (every message_update event as it happens)
 *   3. The API trace (if api-proxy.mjs is running on :9090)
 *
 * Usage:
 *   node scripts/research/capture-pimono-session.mjs [prompt]
 *
 * Prerequisites:
 *   - cd ~/projects/pi-mono && npm run build (or use tsx)
 *   - ANTHROPIC_API_KEY set in environment
 *   - Optional: api-proxy.mjs running on :9090 for API-level logging
 */

import { createAgentSession } from "/Users/maxglassie/projects/pi-mono/packages/coding-agent/src/index.ts";
import { appendFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const PROMPT = process.argv[2] || "Read the file /tmp/test-config.toml and explain what it does. Keep your answer to one paragraph.";
const OUT_DIR = "/tmp/pimono-capture";
const EVENTS_LOG = join(OUT_DIR, "streaming-events.jsonl");
const SESSION_LOG = join(OUT_DIR, "session-meta.json");

// Ensure output dir exists
import { mkdirSync } from "node:fs";
mkdirSync(OUT_DIR, { recursive: true });

// Create a test file for the agent to read
writeFileSync("/tmp/test-config.toml", `[server]
host = "127.0.0.1"
port = 3002

[database]
path = "./data/store.db"
max_connections = 5
`);

writeFileSync(EVENTS_LOG, ""); // clear

console.log("Creating pi-mono agent session...");
console.log(`Prompt: "${PROMPT}"`);
console.log(`Events log: ${EVENTS_LOG}`);
console.log();

const { session } = await createAgentSession({
  noSession: false, // persist JSONL
});

// Subscribe to streaming events — this is the per-block granularity
let eventSeq = 0;
session.subscribe((event) => {
  if (event.type === "message_update") {
    const ame = event.assistantMessageEvent;
    eventSeq++;
    const entry = {
      seq: eventSeq,
      ts: new Date().toISOString(),
      event_type: ame.type,
    };

    // Add type-specific fields
    if ("contentIndex" in ame) entry.contentIndex = ame.contentIndex;
    if ("delta" in ame) entry.delta_length = ame.delta?.length || 0;
    if ("content" in ame) entry.content_length = ame.content?.length || 0;
    if (ame.type === "toolcall_end") {
      entry.tool_name = ame.toolCall?.name;
      entry.tool_id = ame.toolCall?.id;
    }
    if (ame.type === "done") {
      entry.stop_reason = ame.reason;
      entry.content_blocks = event.message?.content?.length;
      entry.content_types = event.message?.content?.map(b => b.type);
    }

    appendFileSync(EVENTS_LOG, JSON.stringify(entry) + "\n");

    // Print a compact summary to stdout
    if (ame.type.endsWith("_start")) {
      const blockType = ame.type.replace("_start", "");
      process.stdout.write(`  [${blockType} start] `);
    } else if (ame.type === "text_delta") {
      process.stdout.write(".");
    } else if (ame.type === "thinking_delta") {
      process.stdout.write("~");
    } else if (ame.type === "toolcall_delta") {
      process.stdout.write("+");
    } else if (ame.type.endsWith("_end")) {
      process.stdout.write(` [end]\n`);
    } else if (ame.type === "done") {
      console.log(`\n  [done] stop_reason=${ame.reason} blocks=${event.message?.content?.length}`);
    }
  }
});

console.log("Sending prompt...\n");
const result = await session.prompt(PROMPT);

// Write session metadata
const meta = {
  prompt: PROMPT,
  content_block_count: result?.content?.length,
  content_types: result?.content?.map(b => b.type),
  stop_reason: result?.stopReason,
  model: result?.model,
  usage: result?.usage,
  streaming_event_count: eventSeq,
};
writeFileSync(SESSION_LOG, JSON.stringify(meta, null, 2) + "\n");

console.log("\n--- Session Summary ---");
console.log(`Content blocks: ${meta.content_block_count}`);
console.log(`Types: ${meta.content_types?.join(", ")}`);
console.log(`Stop reason: ${meta.stop_reason}`);
console.log(`Streaming events captured: ${eventSeq}`);
console.log(`\nOutputs:`);
console.log(`  Streaming events: ${EVENTS_LOG}`);
console.log(`  Session meta: ${SESSION_LOG}`);
console.log(`  API trace: /tmp/api-trace.jsonl (if proxy running)`);

// Find the JSONL session file pi-mono wrote
import { readdirSync, statSync } from "node:fs";
const sessionsDir = join(process.env.HOME, ".pi/agent/sessions");
try {
  const projectDirs = readdirSync(sessionsDir);
  for (const d of projectDirs) {
    const full = join(sessionsDir, d);
    if (!statSync(full).isDirectory()) continue;
    const files = readdirSync(full).filter(f => f.endsWith(".jsonl")).sort();
    if (files.length > 0) {
      const latest = join(full, files[files.length - 1]);
      const age = Date.now() - statSync(latest).mtimeMs;
      if (age < 30000) { // written in last 30s
        console.log(`  Pi-mono JSONL: ${latest}`);
      }
    }
  }
} catch { /* no sessions dir */ }

process.exit(0);
