#!/usr/bin/env node
/**
 * test-pimono-shapes.mjs — Capture diverse pi-mono session shapes for research.
 *
 * Runs a series of controlled prompts through pi-mono's SDK, each designed
 * to produce a different content block combination. For each scenario,
 * captures:
 *   - Streaming events (per-block granularity from subscribe())
 *   - The JSONL session file (pi-mono's bundled persistence)
 *   - API trace (if proxy is running)
 *
 * Each scenario tests a specific hypothesis about how pi-mono bundles
 * content blocks and how that affects Open Story's translator.
 *
 * Usage:
 *   ANTHROPIC_API_KEY=... npx tsx scripts/research/test-pimono-shapes.mjs
 */

import { createAgentSession } from "/Users/maxglassie/projects/pi-mono/packages/coding-agent/src/index.ts";
import { appendFileSync, writeFileSync, mkdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const OUT_DIR = "/tmp/pimono-shapes";
mkdirSync(OUT_DIR, { recursive: true });

// Create test fixtures the agent can interact with
writeFileSync("/tmp/test-config.toml", `[server]
host = "127.0.0.1"
port = 3002
`);
writeFileSync("/tmp/test-broken.py", `def fib(n):
    if n <= 1:
        return n
    return fib(n-1) + fib(n-2)  # exponential time complexity

print(fib(40))  # this will be very slow
`);

// ─── Scenario definitions ──────────────────────────────────────────────

const scenarios = [
  {
    name: "01-text-only",
    prompt: "Say hello in exactly one sentence. Do not use any tools.",
    expect: "Single text block, no tools, no thinking",
    hypothesis: "Assistant message has content: [{type: 'text'}]. Translator should produce message.assistant.text.",
  },
  {
    name: "02-single-tool",
    prompt: "Read the file /tmp/test-config.toml. Only read it, do not explain it.",
    expect: "Single toolCall block, stopReason: toolUse",
    hypothesis: "Assistant message has content: [{type: 'toolCall'}]. Translator picks tool_use — correct.",
  },
  {
    name: "03-tool-then-text",
    prompt: "Read /tmp/test-config.toml and tell me what port the server listens on. One sentence answer.",
    expect: "Two API calls: (1) toolCall, (2) text response",
    hypothesis: "Each API call is one JSONL line. Tool call line is tool_use, text line is text. Both visible — no bug.",
  },
  {
    name: "04-thinking-plus-text",
    prompt: "Think carefully about why 1+1=2, then give me a one sentence answer.",
    expect: "Thinking + text in one response, no tools",
    hypothesis: "Content: [{type:'thinking'}, {type:'text'}]. Translator picks 'thinking', text is INVISIBLE. This is the bug.",
  },
  {
    name: "05-thinking-plus-tool",
    prompt: "Think about what file to read, then read /tmp/test-config.toml.",
    expect: "Thinking + toolCall in one response",
    hypothesis: "Content: [{type:'thinking'}, {type:'toolCall'}]. Translator picks 'tool_use', thinking and any text invisible.",
  },
  {
    name: "06-thinking-text-tool",
    prompt: "Think step by step about what /tmp/test-broken.py does wrong, explain the bug briefly, then read the file.",
    expect: "Thinking + text + toolCall — the full mixed case",
    hypothesis: "Content: [{type:'thinking'}, {type:'text'}, {type:'toolCall'}]. Translator picks 'tool_use'. Both thinking AND text are INVISIBLE. Worst case.",
  },
  {
    name: "07-multi-tool",
    prompt: "Read both /tmp/test-config.toml and /tmp/test-broken.py at the same time.",
    expect: "Multiple toolCall blocks in one response",
    hypothesis: "Content: [{type:'toolCall'}, {type:'toolCall'}]. Translator picks 'tool_use' — but only one tool is captured in the CloudEvent.",
  },
  {
    name: "08-tool-error",
    prompt: "Read the file /tmp/does-not-exist.txt",
    expect: "Tool call that returns an error",
    hypothesis: "toolResult with isError: true. Tests error path preservation.",
  },
];

// ─── Runner ─────────────────────────────────────────────────────────────

async function runScenario(scenario) {
  const scenarioDir = join(OUT_DIR, scenario.name);
  mkdirSync(scenarioDir, { recursive: true });

  const eventsFile = join(scenarioDir, "streaming-events.jsonl");
  const metaFile = join(scenarioDir, "meta.json");
  const jsonlFile = join(scenarioDir, "session.jsonl");
  writeFileSync(eventsFile, "");

  console.log(`\n${"═".repeat(70)}`);
  console.log(`SCENARIO: ${scenario.name}`);
  console.log(`Prompt: "${scenario.prompt}"`);
  console.log(`Expect: ${scenario.expect}`);
  console.log(`Hypothesis: ${scenario.hypothesis}`);
  console.log(`${"─".repeat(70)}`);

  const { session } = await createAgentSession({ noSession: false });

  // Collect streaming events
  const events = [];
  // Track complete messages for analysis
  const completedMessages = [];

  session.subscribe((event) => {
    if (event.type === "message_update") {
      const ame = event.assistantMessageEvent;
      const entry = {
        seq: events.length + 1,
        ts: new Date().toISOString(),
        event_type: ame.type,
      };
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
        entry.content_types = event.message?.content?.map((b) => b.type);
      }
      events.push(entry);
      appendFileSync(eventsFile, JSON.stringify(entry) + "\n");
    } else if (event.type === "message") {
      // Capture the full persisted message
      completedMessages.push(event);
    }
  });

  // Collect all messages the session persists
  const messages = [];
  const origSubscribe = session.subscribe.bind(session);

  // Run the prompt
  let result;
  try {
    result = await session.prompt(scenario.prompt);
  } catch (err) {
    console.log(`  ERROR: ${err.message}`);
    result = { error: err.message };
  }

  // Analyze what happened
  const doneEvents = events.filter((e) => e.event_type === "done");
  const blockStarts = events.filter((e) => e.event_type.endsWith("_start"));
  const apiCalls = doneEvents.length; // each "done" = one API response

  // Collect content block types per API call
  const apiCallBlocks = [];
  let currentBlocks = [];
  for (const e of events) {
    if (e.event_type.endsWith("_start") && !e.event_type.startsWith("tool")) {
      currentBlocks.push(e.event_type.replace("_start", ""));
    } else if (e.event_type === "toolcall_start") {
      currentBlocks.push("toolcall");
    } else if (e.event_type === "done") {
      apiCallBlocks.push([...currentBlocks]);
      currentBlocks = [];
    }
  }

  const meta = {
    scenario: scenario.name,
    prompt: scenario.prompt,
    hypothesis: scenario.hypothesis,
    results: {
      api_calls: apiCalls,
      total_streaming_events: events.length,
      block_starts: blockStarts.map((e) => e.event_type.replace("_start", "")),
      per_api_call_blocks: apiCallBlocks,
      done_events: doneEvents.map((e) => ({
        stop_reason: e.stop_reason,
        content_blocks: e.content_blocks,
        content_types: e.content_types,
      })),
    },
    // What the translator would do today
    translator_analysis: analyzeForTranslator(doneEvents),
  };

  writeFileSync(metaFile, JSON.stringify(meta, null, 2) + "\n");

  // Print results
  console.log(`  API calls: ${apiCalls}`);
  console.log(`  Streaming events: ${events.length}`);
  for (let i = 0; i < apiCallBlocks.length; i++) {
    const done = doneEvents[i];
    const types = done?.content_types?.join(", ") || "?";
    const stop = done?.stop_reason || "?";
    console.log(`  Call ${i + 1}: [${apiCallBlocks[i].join(", ")}] → content_types=[${types}] stop=${stop}`);

    // What translator would pick
    const picked = pickSubtype(done?.content_types || []);
    const lost = (done?.content_types || []).filter((t) => t !== mapToType(picked));
    if (lost.length > 0) {
      console.log(`    ⚠ Translator picks: ${picked} → INVISIBLE: ${lost.join(", ")}`);
    } else {
      console.log(`    ✓ Translator picks: ${picked} → nothing lost`);
    }
  }

  // Find and copy the JSONL session file pi-mono wrote
  try {
    const sessionsDir = join(process.env.HOME, ".pi/agent/sessions");
    const { readdirSync: rd, statSync: st, copyFileSync: cp } = await import("node:fs");
    for (const d of rd(sessionsDir)) {
      const full = join(sessionsDir, d);
      if (!st(full).isDirectory()) continue;
      const files = rd(full).filter((f) => f.endsWith(".jsonl")).sort();
      if (files.length > 0) {
        const latest = join(full, files[files.length - 1]);
        const age = Date.now() - st(latest).mtimeMs;
        if (age < 30000) {
          cp(latest, jsonlFile);
          console.log(`  JSONL saved: ${jsonlFile}`);
        }
      }
    }
  } catch { /* no sessions dir */ }

  return meta;
}

// Simulate what translate_pi.rs does today
function pickSubtype(contentTypes) {
  if (!contentTypes || contentTypes.length === 0) return "message.assistant.text";
  if (contentTypes.includes("toolCall")) return "message.assistant.tool_use";
  if (contentTypes.includes("thinking")) return "message.assistant.thinking";
  return "message.assistant.text";
}

function mapToType(subtype) {
  if (subtype === "message.assistant.tool_use") return "toolCall";
  if (subtype === "message.assistant.thinking") return "thinking";
  return "text";
}

function analyzeForTranslator(doneEvents) {
  return doneEvents.map((e) => {
    const types = e.content_types || [];
    const picked = pickSubtype(types);
    const visible = [mapToType(picked)];
    const invisible = types.filter((t) => !visible.includes(t));
    return {
      content_types: types,
      translator_picks: picked,
      visible,
      invisible,
      data_loss: invisible.length > 0,
    };
  });
}

// ─── Main ───────────────────────────────────────────────────────────────

console.log("╔══════════════════════════════════════════════════════════════════════╗");
console.log("║  Pi-Mono Shape Capture — mapping the content block problem space   ║");
console.log("╚══════════════════════════════════════════════════════════════════════╝");

const results = [];
for (const scenario of scenarios) {
  try {
    const meta = await runScenario(scenario);
    results.push(meta);
  } catch (err) {
    console.log(`  FATAL: ${err.message}`);
    results.push({ scenario: scenario.name, error: err.message });
  }
}

// ─── Summary ────────────────────────────────────────────────────────────

console.log(`\n${"═".repeat(70)}`);
console.log("SUMMARY: Translator data loss by scenario");
console.log(`${"═".repeat(70)}`);

for (const r of results) {
  if (r.error) {
    console.log(`  ${r.scenario}: ERROR — ${r.error}`);
    continue;
  }
  const analyses = r.translator_analysis || [];
  const hasLoss = analyses.some((a) => a.data_loss);
  const status = hasLoss ? "⚠ DATA LOSS" : "✓ OK";
  const details = analyses
    .filter((a) => a.data_loss)
    .map((a) => `invisible: [${a.invisible.join(",")}]`)
    .join("; ");
  console.log(`  ${r.scenario}: ${status} ${details}`);
}

// Write full results
writeFileSync(join(OUT_DIR, "summary.json"), JSON.stringify(results, null, 2) + "\n");
console.log(`\nFull results: ${OUT_DIR}/summary.json`);
console.log(`Per-scenario: ${OUT_DIR}/<scenario>/meta.json`);

process.exit(0);
