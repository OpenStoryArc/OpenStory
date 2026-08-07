#!/usr/bin/env node
/**
 * Full click-parity survey — walk the ActionGraph + every canvas mode.
 *
 *   pairs  = allReachablePairs(ENTITY_EDGES)  (sampled multi-hop + all direct edges)
 *   modes  = CANVAS_MODES × select session
 *   land   = location.hash after each control step
 *
 * Usage: node scripts/nav_path.mjs
 * Needs: :3002, :5173, playwright (e2e/)
 */

import { createRequire } from "node:module";
import { writeFileSync } from "node:fs";

const require = createRequire(new URL("../e2e/package.json", import.meta.url));
const { chromium } = require("playwright");

const API = "http://127.0.0.1:3002";
const UI = "http://127.0.0.1:5173";
const REPORT = "docs/reports/nav-path.md";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const CANVAS_MODES = [
  "sunburst", "board", "treemap", "gantt", "scatter", "flow",
  "tool-adjacency", "agent-project", "durations", "heatmap",
];

const ENTITY_EDGES = [
  ["person", "session", "query"],
  ["project", "session", "query"],
  ["session", "subagent", "open_view"],
  ["session", "turn", "open_view"],
  ["session", "plan", "open_view"],
  ["session", "event", "open_view"],
  ["turn", "sentence", "set"],
  ["turn", "event", "focus_event"],
  ["event", "turn", "focus_event"],
  ["subagent", "session", "open_view"],
  ["toolcall", "result", "focus_event"],
  ["toolcall", "file", "open_view"],
  ["file", "session", "query"],
  ["error", "event", "focus_event"],
  ["plan", "turn", "focus_event"],
];

function planDirect(from, to, ctx) {
  const key = `${from}>${to}`;
  const { sid, eid, user, project, file } = ctx;
  const plans = {
    "person>session": () => [{ action: "query", params: { user }, land: /user=/ }],
    "project>session": () => [{ action: "query", params: { project }, land: /project=/ }],
    "session>turn": () => [{ action: "open_view", params: { view: "story", sessionId: sid }, land: new RegExp(`story/${sid}`) }],
    "session>event": () => [{ action: "focus_event", params: { sessionId: sid, eventId: eid, view: "explore" }, land: new RegExp(`event/${eid}`) }],
    "session>plan": () => [{ action: "open_view", params: { view: "explore", sessionId: sid, detailView: "plans" }, land: new RegExp(`explore/${sid}`) }],
    "session>subagent": () => [{ action: "open_view", params: { view: "explore", sessionId: sid, detailView: "graph" }, land: new RegExp(`explore/${sid}`) }],
    "event>turn": () => [{ action: "focus_event", params: { sessionId: sid, eventId: eid, view: "story" }, land: new RegExp(`story/${sid}/event/${eid}`) }],
    "turn>event": () => [{ action: "focus_event", params: { sessionId: sid, eventId: eid, view: "explore" }, land: new RegExp(`event/${eid}`) }],
    "turn>sentence": () => [
      { action: "focus_event", params: { sessionId: sid, eventId: eid, view: "story" }, land: new RegExp(`story/${sid}/event/${eid}`) },
      { action: "set", params: { target: "story.details", open: true, sessionId: sid, eventId: eid }, land: /details=1/ },
    ],
    "toolcall>file": () => [{ action: "open_view", params: { view: "explore", detailView: "search", searchQuery: file }, land: /search\?q=/ }],
    "file>session": () => [{ action: "open_view", params: { view: "explore", detailView: "search", searchQuery: file }, land: /search\?q=/ }],
    "toolcall>result": () => [{ action: "focus_event", params: { sessionId: sid, eventId: eid, view: "explore" }, land: new RegExp(`event/${eid}`) }],
    "error>event": () => [{ action: "focus_event", params: { sessionId: sid, eventId: eid, view: "explore" }, land: new RegExp(`event/${eid}`) }],
    "plan>turn": () => [{ action: "focus_event", params: { sessionId: sid, eventId: eid, view: "story" }, land: new RegExp(`story/${sid}/event/${eid}`) }],
    "subagent>session": () => [{ action: "open_view", params: { view: "explore", sessionId: sid }, land: new RegExp(`explore/${sid}`) }],
  };
  return plans[key]?.() ?? null;
}

const b = await chromium.launch({
  headless: true,
  args: ["--no-sandbox", "--disable-dev-shm-usage", "--disable-gpu"],
});
const p = await b.newPage({ viewport: { width: 1400, height: 900 } });
await p.goto(`${UI}/#/explore`, { waitUntil: "networkidle" });
await sleep(2000);

const { sid, eid } = await p.evaluate(async () => {
  const list = await fetch("/api/sessions?limit=40").then((r) => r.json());
  const s = list.sessions.find((x) => x.event_count > 40 && !x.session_id.startsWith("smoke"));
  const recs = await fetch(`/api/sessions/${s.session_id}/records`).then((r) => r.json());
  const arr = Array.isArray(recs) ? recs : [];
  const rec = arr.find((e) => e.id && !String(e.id).includes(":"));
  return { sid: s.session_id, eid: rec?.id ?? null };
});

const ctx = { sid, eid, user: "max", project: "OpenStory", file: "App.tsx" };
console.log(`\n🧭 full nav_path — ${sid.slice(0, 8)} / ${String(eid).slice(0, 8)}\n`);

async function ctrl(action, params) {
  await p.request.post(`${API}/api/control`, {
    headers: { "content-type": "application/json" },
    data: { action, params, issuer: "nav-path" },
  });
}

const results = [];

// 1) Every direct ActionGraph edge
for (const [from, to] of ENTITY_EDGES.map((e) => [e[0], e[1]])) {
  const steps = planDirect(from, to, ctx);
  if (!steps) {
    results.push({ kind: "edge", from, to, ok: false, note: "no plan" });
    console.log(`  ❌ edge ${from}→${to}`);
    continue;
  }
  await ctrl("open_view", { view: "explore" });
  await sleep(350);
  let ok = true;
  let last = "";
  for (const step of steps) {
    await ctrl(step.action, step.params);
    await sleep(850);
    last = await p.evaluate(() => location.hash);
    if (step.land && !step.land.test(last)) {
      ok = false;
      console.log(`  ❌ edge ${from}→${to} @ ${step.action}: ${last}`);
      break;
    }
  }
  if (ok) console.log(`  ✅ edge ${from}→${to} (${steps.length}) → ${last.slice(0, 60)}`);
  results.push({ kind: "edge", from, to, ok, steps: steps.length, hash: last });
}

// 2) Multi-hop: event → sentence, person → event
const multi = [
  ["event", "sentence", () => [
    { action: "focus_event", params: { sessionId: sid, eventId: eid, view: "story" }, land: new RegExp(`story/${sid}/event/${eid}`) },
    { action: "set", params: { target: "story.details", open: true, sessionId: sid, eventId: eid }, land: /details=1/ },
  ]],
  ["person", "session", () => [{ action: "query", params: { user: "max" }, land: /user=/ }]],
];
for (const [from, to, build] of multi) {
  const steps = build();
  await ctrl("open_view", { view: "explore" });
  await sleep(350);
  let ok = true;
  let last = "";
  for (const step of steps) {
    await ctrl(step.action, step.params);
    await sleep(850);
    last = await p.evaluate(() => location.hash);
    if (step.land && !step.land.test(last)) {
      ok = false;
      break;
    }
  }
  console.log(`  ${ok ? "✅" : "❌"} multi ${from}→${to}`);
  results.push({ kind: "multi", from, to, ok, steps: steps.length, hash: last });
}

// 3) Every canvas mode + select session
for (const mode of CANVAS_MODES) {
  await ctrl("navigate_to", { kind: "session", id: sid, canvasMode: mode });
  await sleep(1200);
  const h = await p.evaluate(() => location.hash);
  const ok = /#\/canvas/.test(h);
  console.log(`  ${ok ? "✅" : "❌"} canvas ${mode} → ${h}`);
  results.push({ kind: "canvas", mode, ok, hash: h });
}

// 4) navigate_to day
await ctrl("navigate_to", { kind: "day", id: "2026-07-27" });
await sleep(900);
const dayHash = await p.evaluate(() => location.hash);
const dayOk = /day=2026-07-27/.test(dayHash);
console.log(`  ${dayOk ? "✅" : "❌"} day filter → ${dayHash}`);
results.push({ kind: "day", ok: dayOk, hash: dayHash });

// 5) Story full expand (details + eval + events)
await ctrl("navigate_to", {
  kind: "event",
  id: eid,
  sessionId: sid,
  expandAll: true,
});
await sleep(1200);
const expHash = await p.evaluate(() => location.hash);
const expOk = /details=1/.test(expHash) && /eval=1/.test(expHash) && /events=1/.test(expHash);
console.log(`  ${expOk ? "✅" : "❌"} story expandAll → ${expHash}`);
results.push({ kind: "expand", ok: expOk, hash: expHash });

await b.close();

const passed = results.filter((r) => r.ok).length;
let md = `# Full click-parity nav survey\n\n`;
md += `_scripts/nav_path.mjs — ActionGraph edges + canvas modes + day._\n\n`;
md += `**${passed}/${results.length} landed**\n\n`;
md += `| kind | path | ok | hash |\n|---|---|:--:|---|\n`;
for (const r of results) {
  const path = r.mode ? `canvas:${r.mode}` : `${r.from || ""}→${r.to || r.kind}`;
  md += `| ${r.kind} | ${path} | ${r.ok ? "✅" : "❌"} | \`${(r.hash || r.note || "").slice(0, 70)}\` |\n`;
}
writeFileSync(REPORT, md);
console.log(`\n${passed}/${results.length} → ${REPORT}\n`);
process.exit(passed === results.length ? 0 : 1);
