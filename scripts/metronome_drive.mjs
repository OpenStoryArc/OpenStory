#!/usr/bin/env node
/**
 * metronome_drive.mjs — the METRONOME (Phase 2b of the agent-in-UI perf harness).
 *
 * Replays the Cool Cat interaction "score" (mirror of ui/src/lib/interaction-score.ts)
 * at escalating tempo and measures where the seam BENDS:
 *   - per-PRIMITIVE request latency (p50/p95/p99) at each tempo
 *   - ui-state FRESHNESS (POST → visible on GET /api/ui-state)
 *   - the KNEE: the first tempo where a metric breaches its NFR
 *
 * Sovereignty: only drives ui.* (POST /api/control + /api/interactions); never
 * touches the observed read-only events.* path.
 *
 * Usage: node scripts/metronome_drive.mjs [--bars 8] [--base http://localhost:3002] [--report path]
 */

import { writeFileSync } from "node:fs";
import { performance } from "node:perf_hooks";

// ── flags ──
const argv = process.argv.slice(2);
const flag = (n, d) => { const i = argv.indexOf(`--${n}`); return i >= 0 && i + 1 < argv.length ? argv[i + 1] : d; };
const BARS = Number(flag("bars", "8"));
const BASE = flag("base", "http://localhost:3002");
const REPORT = flag("report", "docs/reports/perf-metronome.md");

// ── mirror of ui/src/lib/interaction-score.ts ──
const PRIMITIVES = ["open_view", "focus_event", "toggle", "query", "present", "interaction"];
const RIFF = ["open_view", "toggle", "focus_event", "query", "open_view", "present", "focus_event", "interaction"];
const VIEWS = ["overview", "canvas", "story", "explore", "heatmap", "lab"];
const MODES = ["board", "sunburst", "treemap", "scatter", "gantt", "flow"];
const FACETS = ["project", "agent", "status", "host"];
const SESSIONS = ["demo-a", "demo-b", "demo-c"];

function paramsFor(p, i) {
  switch (p) {
    case "open_view": return { view: VIEWS[i % VIEWS.length] };
    case "focus_event": return { sessionId: SESSIONS[i % SESSIONS.length], eventId: `evt-${i}`, view: "explore" };
    case "toggle": return { target: "canvas.mode", value: MODES[i % MODES.length] };
    case "query": return { [FACETS[i % FACETS.length]]: `probe-${i}` };
    case "present": return { message: `metronome beat ${i}`, issuer: "metronome" };
    case "interaction": return { kind: "navigate", view: VIEWS[i % VIEWS.length], issuer: "metronome" };
  }
}
function interactionScore(bpm, bars) {
  const beatMs = 60000 / (bpm > 0 ? bpm : 1);
  const total = Math.max(0, Math.floor(bars)) * 4;
  const beats = [];
  for (let i = 0; i < total; i++) {
    const action = RIFF[i % RIFF.length];
    beats.push({ atMs: i * beatMs, action, params: paramsFor(action, i) });
  }
  return beats;
}

// ── endpoint routing: interaction → /api/interactions; rest → /api/control ──
const endpointFor = (a) => (a === "interaction" ? "/api/interactions" : "/api/control");
const bodyFor = (b) => (b.action === "interaction" ? b.params : { action: b.action, params: b.params, issuer: "metronome" });

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function timedPost(beat) {
  const t0 = performance.now();
  let ok = false;
  try {
    const res = await fetch(BASE + endpointFor(beat.action), {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(bodyFor(beat)),
    });
    ok = res.ok;
    await res.text();
  } catch { ok = false; }
  return { primitive: beat.action, ms: performance.now() - t0, ok };
}

function pct(sorted, p) {
  if (!sorted.length) return 0;
  const idx = Math.min(sorted.length - 1, Math.ceil((p / 100) * sorted.length) - 1);
  return sorted[Math.max(0, idx)];
}

async function runTempo({ bpm, burst }) {
  const score = interactionScore(bpm, BARS);
  let results;
  if (burst) {
    // fire-as-fast-as-possible: max contention at the top of the ramp
    results = await Promise.all(score.map(timedPost));
  } else {
    const proms = [];
    const start = performance.now();
    for (const b of score) {
      const wait = b.atMs - (performance.now() - start);
      if (wait > 0) await sleep(wait);
      proms.push(timedPost(b));
    }
    results = await Promise.all(proms);
  }
  // group latencies by primitive
  const byPrim = {};
  for (const p of PRIMITIVES) byPrim[p] = [];
  for (const r of results) byPrim[r.primitive].push(r.ms);
  const stats = {};
  for (const p of PRIMITIVES) {
    const s = byPrim[p].slice().sort((a, b) => a - b);
    stats[p] = { n: s.length, p50: pct(s, 50), p95: pct(s, 95), p99: pct(s, 99) };
  }
  const all = results.map((r) => r.ms).sort((a, b) => a - b);
  stats._all = { n: all.length, p50: pct(all, 50), p95: pct(all, 95), p99: pct(all, 99) };
  return stats;
}

// ui-state freshness: POST a uniquely-marked interaction, poll GET /api/ui-state
// until it reflects the marker, measure POST→visible lag.
async function measureFreshness() {
  const sid = `fresh-${Math.floor(performance.now())}-${Math.random().toString(36).slice(2, 8)}`;
  const t0 = performance.now();
  await fetch(BASE + "/api/interactions", {
    method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ kind: "navigate", view: "canvas", session_id: sid, issuer: "metronome-fresh" }),
  });
  for (let i = 0; i < 400; i++) {
    const r = await fetch(BASE + "/api/ui-state");
    const j = await r.json();
    if (j?.ui_state?.session_id === sid) return performance.now() - t0;
    await sleep(2);
  }
  return -1; // never became visible
}

const NFR = { control: 100, interaction: 50, freshness: 200 };
const fmt = (n) => (n < 0 ? "—" : n.toFixed(1));

async function main() {
  const ramp = [
    { label: "1x", bpm: 120, burst: false },
    { label: "10x", bpm: 1200, burst: false },
    { label: "100x", bpm: 12000, burst: true },
  ];
  const rows = [];
  console.log(`\n🥁 metronome — Cool Cat @ ${BARS} bars (${BARS * 4} beats/tempo), base ${BASE}\n`);
  for (const t of ramp) {
    const stats = await runTempo(t);
    const fresh = await measureFreshness();
    rows.push({ tempo: t.label, bpm: t.bpm, stats, fresh });
    console.log(`${t.label.padEnd(5)} (${t.bpm}bpm) all p95=${fmt(stats._all.p95)}ms p99=${fmt(stats._all.p99)}ms · freshness=${fmt(fresh)}ms`);
  }

  // ── knees: first tempo breaching each NFR ──
  const controlP95 = (r) => Math.max(...PRIMITIVES.filter((p) => p !== "interaction").map((p) => r.stats[p].p95));
  const knee = (test) => rows.find(test)?.tempo ?? "none (holds through 100x)";
  const controlKnee = knee((r) => controlP95(r) > NFR.control);
  const interactionKnee = knee((r) => r.stats.interaction.p95 > NFR.interaction);
  const freshnessKnee = knee((r) => r.fresh < 0 || r.fresh > NFR.freshness);

  // ── markdown report ──
  let md = `# Metronome perf sweep — agent-in-UI write seam\n\n`;
  md += `_Cool Cat groove replayed at escalating tempo. ${BARS} bars (${BARS * 4} beats) per tempo. Generated by \`scripts/metronome_drive.mjs\`._\n\n`;
  md += `**NFRs:** control p95 < ${NFR.control}ms · POST /api/interactions p95 < ${NFR.interaction}ms · ui-state freshness < ${NFR.freshness}ms.\n\n`;
  md += `## Knees (first tempo to breach)\n\n`;
  md += `| Metric | NFR | Knee |\n|---|---|---|\n`;
  md += `| control p95 | < ${NFR.control}ms | ${controlKnee} |\n`;
  md += `| interaction p95 | < ${NFR.interaction}ms | ${interactionKnee} |\n`;
  md += `| ui-state freshness | < ${NFR.freshness}ms | ${freshnessKnee} |\n\n`;
  md += `## Per-primitive latency (p50 / p95 / p99 ms)\n\n`;
  for (const r of rows) {
    md += `### ${r.tempo} — ${r.bpm} bpm\n\n`;
    md += `| primitive | n | p50 | p95 | p99 | NFR | pass |\n|---|--:|--:|--:|--:|--:|:--:|\n`;
    for (const p of PRIMITIVES) {
      const s = r.stats[p];
      const nfr = p === "interaction" ? NFR.interaction : NFR.control;
      const pass = s.p95 <= nfr ? "✅" : "❌";
      md += `| ${p} | ${s.n} | ${fmt(s.p50)} | ${fmt(s.p95)} | ${fmt(s.p99)} | ${nfr} | ${pass} |\n`;
    }
    md += `| **all** | ${r.stats._all.n} | ${fmt(r.stats._all.p50)} | ${fmt(r.stats._all.p95)} | ${fmt(r.stats._all.p99)} | — | — |\n`;
    md += `\n_ui-state freshness: **${fmt(r.fresh)}ms** (NFR < ${NFR.freshness}ms — ${r.fresh >= 0 && r.fresh <= NFR.freshness ? "✅" : "❌"})_\n\n`;
  }
  md += `## Notes\n\n`;
  md += `- \`delivered\`/dropped-broadcast measurement needs a connected WS sink (a browser or a node WS client counting received vs \`delivered\`); not measured here — tracked for a follow-up.\n`;
  md += `- 1x spaces beats 500ms apart (no concurrency → true baseline latency); 10x overlaps; 100x is a fire-at-once burst (max contention).\n`;

  writeFileSync(REPORT, md);
  console.log(`\nKnees → control:${controlKnee} · interaction:${interactionKnee} · freshness:${freshnessKnee}`);
  console.log(`Report written: ${REPORT}\n`);
}

main().catch((e) => { console.error(e); process.exit(1); });
