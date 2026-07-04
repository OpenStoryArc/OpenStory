#!/usr/bin/env node
/**
 * nav_survey.mjs — the MCP surveyor. Walks the ActionGraph and proves every
 * movement LANDS somewhere. This is the map principle as a conformance test:
 * a dead end isn't a vibe, it's an edge that fails to land.
 *
 * How: a real browser (the sink) connects over WS; for each DRIVABLE edge we
 * POST the control action to the seam (exactly what the MCP does) and read the
 * resulting route. Lands → PASS. Doesn't → a dead end we can SEE. Edges with no
 * verb (via:null) are structural dead ends, listed as gaps.
 *
 * Mirrors ui/src/lib/action-graph.ts (standalone, like replay_journey.mjs).
 * Sovereignty: drives only ui.* (/api/control); never events.*.
 *
 * Usage: node scripts/nav_survey.mjs   (needs server 3002 + vite 5173 + chromium)
 */

import { createRequire } from "node:module";
import { writeFileSync } from "node:fs";
// Playwright is an e2e/ dev-dep; resolve it from there so this script in
// scripts/ runs from anywhere (Node resolves imports relative to the script).
const require = createRequire(new URL("../e2e/package.json", import.meta.url));
const { chromium } = require("playwright");

const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const API = "http://127.0.0.1:3002";
const UI = "http://127.0.0.1:5173";
const REPORT = "docs/reports/navigability.md";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ── the drivable edges (mirror of ENTITY_EDGES where via is a real verb) ──
// each: how to drive it, and what "landed" looks like in the route.
function drivableEdges(sid, eid) {
  return [
    { edge: "person→session", via: "query", ctrl: ["query", { user: "max" }], lands: /overview/ },
    { edge: "project→session", via: "query", ctrl: ["query", { project: "OpenStory" }], lands: /overview/ },
    { edge: "session→subagent", via: "open_view", ctrl: ["open_view", { view: "explore", sessionId: sid, detailView: "graph" }], lands: new RegExp(`explore/${sid}`) },
    { edge: "session→turn", via: "open_view", ctrl: ["open_view", { view: "story", sessionId: sid }], lands: new RegExp(`story/${sid}`) },
    { edge: "session→plan", via: "open_view", ctrl: ["open_view", { view: "explore", sessionId: sid, detailView: "plans" }], lands: new RegExp(`explore/${sid}`) },
    { edge: "session→event", via: "open_view", ctrl: ["open_view", { view: "explore", sessionId: sid }], lands: new RegExp(`explore/${sid}`) },
    { edge: "turn→event", via: "focus_event", ctrl: ["focus_event", { sessionId: sid, eventId: eid, view: "explore" }], lands: new RegExp(`event/${eid}`) },
    { edge: "event→turn", via: "focus_event", ctrl: ["focus_event", { sessionId: sid, eventId: eid, view: "story" }], lands: new RegExp(`story/${sid}/event/${eid}`) },
    { edge: "file→session", via: "query", ctrl: ["open_view", { view: "explore", detailView: "search", searchQuery: "App.tsx" }], lands: /search\?q=App/ },
    { edge: "subagent→session", via: "open_view", ctrl: ["open_view", { view: "explore", sessionId: sid }], lands: new RegExp(`explore/${sid}`) },
  ];
}
// structural dead ends (via:null) — no verb walks them yet.
const DEAD_ENDS = [
  "toolcall→result (paired)",
  "toolcall→file (writes)", "error→event (locus)", "plan→turn (authored by)",
];

// Chromium: prefer OPEN_STORY_CHROME, else the legacy snapshot path, else
// Playwright's own installed browser (npx playwright install chromium).
const launchOpts = { headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage", "--disable-gpu"] };
const chromePath = process.env.OPEN_STORY_CHROME ?? `${S}/chrome-linux/chrome`;
const b = await chromium
  .launch({ ...launchOpts, executablePath: chromePath })
  .catch(() => chromium.launch(launchOpts));
const p = await b.newPage({ viewport: { width: 1400, height: 900 } });
p.on("pageerror", (e) => console.log("PAGEERR:", e.message.slice(0, 120)));

await p.goto(`${UI}/#/overview`, { waitUntil: "networkidle" });
await sleep(2500);

// pick a real session + a real (non-:usage) event in it
const { sid, eid } = await p.evaluate(async () => {
  const list = await fetch("/api/sessions?limit=60").then((r) => r.json());
  const s = list.sessions.find((x) => x.event_count > 60 && !x.session_id.startsWith("smoke"));
  const recs = await fetch(`/api/sessions/${s.session_id}/records`).then((r) => r.json());
  const rec = (Array.isArray(recs) ? recs : []).find((e) => e.id && !e.id.includes(":"));
  return { sid: s.session_id, eid: rec?.id ?? null };
});
console.log(`\n🧭 nav survey — session ${sid.slice(0, 8)}, event ${eid?.slice(0, 8)}\n`);

async function ctrl(action, params) {
  await p.request.post(`${API}/api/control`, { headers: { "content-type": "application/json" }, data: { action, params, issuer: "nav-survey" } });
}
const hash = () => p.evaluate(() => location.hash);

const results = [];
for (const e of drivableEdges(sid, eid)) {
  await ctrl("open_view", { view: "overview" }); await sleep(500); // neutral reset
  await ctrl(e.ctrl[0], e.ctrl[1]);
  await sleep(900);
  const h = await hash();
  const landed = e.lands.test(h);
  results.push({ ...e, landed, hash: h });
  console.log(`  ${landed ? "✅" : "❌"} ${e.edge.padEnd(18)} via ${e.via.padEnd(11)} → ${h}`);
}

await b.close();

const passed = results.filter((r) => r.landed).length;
const walkable = drivableEdges("x", "y").length;
const totalEdges = walkable + DEAD_ENDS.length + 1; // +1 inherent (turn→sentence)
const coverage = ((walkable + 1) / totalEdges * 100).toFixed(0);

// ── report ──
let md = `# Navigability survey — the ActionGraph, walked live\n\n`;
md += `_The MCP surveyor drives every drivable edge through the control seam and checks it LANDS. Generated by \`scripts/nav_survey.mjs\`; graph from \`ui/src/lib/action-graph.ts\`._\n\n`;
md += `**Coverage: ${walkable + 1}/${totalEdges} edges realized (${coverage}%)** — ${passed}/${walkable} drivable edges landed live; ${DEAD_ENDS.length} dead ends have no verb yet.\n\n`;
md += `## Drivable edges (walked live via the MCP)\n\n| edge | verb | landed? | route |\n|---|---|:--:|---|\n`;
for (const r of results) md += `| ${r.edge} | ${r.via} | ${r.landed ? "✅" : "❌"} | \`${r.hash}\` |\n`;
md += `\n## Dead ends (no verb walks them — the branches to grow)\n\n`;
for (const d of DEAD_ENDS) md += `- ${d}\n`;
md += `\n_A dead end is an edge the data has that no control verb realizes. Each one is a \`navigateToEntity\` we haven't grown. Close one → it moves up to the table above and gets a live landing test for free._\n`;
writeFileSync(REPORT, md);

console.log(`\nDrivable edges landed: ${passed}/${walkable} · dead ends: ${DEAD_ENDS.length} · coverage ~${coverage}%`);
console.log(`Report: ${REPORT}\n`);
