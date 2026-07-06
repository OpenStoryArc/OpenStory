#!/usr/bin/env node
/**
 * replay_journey.mjs — the REPLAY DRIVER (Phase 4b of the agent-in-UI seam).
 *
 * interaction ↔ command are INVERSES. This script closes the loop:
 *   1. GET /api/ui-state/journey  — the human's captured PATH (interaction stream)
 *   2. replay(interactions, {direction, tempo})  — the SAME pure engine the UI
 *      would use (mirrored here so the driver is standalone), turning each
 *      interaction into its inverse control step
 *   3. POST each step to /api/control on its atMs schedule — the dashboard
 *      RETRACES (forward) or REWINDS (backward) the journey, live.
 *
 * Sovereignty: this only drives the mirror (control → ui.*). It never touches the
 * observed read-only events.* namespace.
 *
 * Usage:
 *   node scripts/replay_journey.mjs [--direction forward|backward] [--tempo N]
 *                                   [--n N] [--issuer NAME] [--base URL] [--dry]
 */

const args = process.argv.slice(2);
function flag(name, def) {
  const i = args.indexOf(`--${name}`);
  return i >= 0 && i + 1 < args.length ? args[i + 1] : def;
}
const DIRECTION = flag("direction", "forward");
const TEMPO = Number(flag("tempo", "1"));
const N = Number(flag("n", "20"));
const ISSUER = flag("issuer", "replay");
const BASE = flag("base", "http://localhost:3002");
const DRY = args.includes("--dry");

const BASE_STEP_MS = 1000;

/** Mirror of ui/src/lib/replay.ts::toControl — one interaction → inverse control. */
function toControl(i) {
  switch (i.kind) {
    case "navigate": {
      const params = { view: i.view };
      if (i.session_id) params.sessionId = i.session_id;
      if (i.detailView) params.detailView = i.detailView;
      if (i.eventId) params.eventId = i.eventId;
      return { action: "open_view", params };
    }
    case "select":
      return i.eventId
        ? { action: "focus_event", params: { sessionId: i.session_id, eventId: i.eventId, view: i.view } }
        : { action: "open_view", params: { view: i.view, sessionId: i.session_id } };
    case "zoom":
      return i.mode ? { action: "toggle", params: { target: "canvas.mode", value: i.mode } } : null;
    case "filter":
      return { action: "query", params: { ...(i.filters ?? {}) } };
    default:
      return null;
  }
}

/** Mirror of ui/src/lib/replay.ts::replay. */
function replay(interactions, { direction, tempo }) {
  const ordered = direction === "backward" ? [...interactions].reverse() : interactions;
  const t = tempo > 0 ? tempo : 1;
  const gap = BASE_STEP_MS / t;
  const steps = [];
  for (const i of interactions ? ordered : []) {
    const ctrl = toControl(i);
    if (!ctrl) continue;
    steps.push({ atMs: steps.length * gap, action: ctrl.action, params: ctrl.params });
  }
  return steps;
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function main() {
  const res = await fetch(`${BASE}/api/ui-state/journey?n=${N}`);
  const { journey } = await res.json();
  if (!Array.isArray(journey) || journey.length === 0) {
    console.error("No captured journey to replay. Drive the UI first (interactions land on ui.*).");
    process.exit(1);
  }
  console.log(`Captured journey: ${journey.length} interactions`);
  journey.forEach((i, n) => console.log(`  ${n + 1}. ${i.kind}${i.view ? ` (${i.view}${i.session_id ? ` · ${i.session_id.slice(0, 8)}` : ""}${i.eventId ? ` · ${i.eventId.slice(0, 8)}` : ""})` : ""}`));

  const steps = replay(journey, { direction: DIRECTION, tempo: TEMPO });
  console.log(`\nReplaying ${DIRECTION.toUpperCase()} @ tempo ${TEMPO} — ${steps.length} control steps:`);

  let prev = 0;
  for (const step of steps) {
    await sleep(step.atMs - prev);
    prev = step.atMs;
    const label = `[${(step.atMs / 1000).toFixed(1)}s] ${step.action} ${JSON.stringify(step.params)}`;
    if (DRY) {
      console.log(`  (dry) ${label}`);
      continue;
    }
    console.log(`  ${label}`);
    await fetch(`${BASE}/api/control`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ action: step.action, params: step.params, issuer: ISSUER }),
    }).catch((e) => console.error("  POST failed:", e.message));
  }
  console.log(`\nDone — journey ${DIRECTION === "backward" ? "rewound" : "retraced"}.`);
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});
