import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const API = "http://127.0.0.1:3002";
const SESS = "2a0d4337-4d8a-4bfd-8d94-3c62c164568c";
const EVT = "fca31b2f-d1fd-42c1-afe2-ad472328554e";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// --- mirror of ui/src/lib/replay.ts (kept standalone for the driver) ---
const BASE_STEP_MS = 1000;
function toControl(i) {
  switch (i.kind) {
    case "navigate": {
      const params = { view: i.view };
      if (i.session_id) params.sessionId = i.session_id;
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
function replay(interactions, { direction, tempo }) {
  const ordered = direction === "backward" ? [...interactions].reverse() : interactions;
  const gap = BASE_STEP_MS / (tempo > 0 ? tempo : 1);
  const steps = [];
  for (const i of ordered) {
    const c = toControl(i);
    if (!c) continue;
    steps.push({ atMs: steps.length * gap, action: c.action, params: c.params });
  }
  return steps;
}

const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage", "--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", (e) => console.log("PAGEERR:", e.message.slice(0, 160)));

async function ctrl(action, params) {
  const res = await p.request.post(`${API}/api/control`, { headers: { "content-type": "application/json" }, data: { action, params, issuer: "replay" } });
  return res.json();
}

// 1. Sit on Overview, WS connected.
await p.goto("http://127.0.0.1:5173/#/overview", { waitUntil: "networkidle" });
await p.waitForTimeout(2500);

// 2. CAPTURE a fresh journey by driving 3 distinct locations. Each navigation
//    makes the UI (a sink) post an interaction to ui.* — that's the captured path.
console.log("=== CAPTURE ===");
await ctrl("open_view", { view: "canvas" });
await p.waitForTimeout(1400);
await ctrl("focus_event", { sessionId: SESS, eventId: EVT, view: "explore" });
await p.waitForTimeout(1600);
console.log("captured end hash:", await p.evaluate(() => location.hash));
await p.screenshot({ path: `${S}/replay-captured.png` });

// 3. Read the captured journey back (the same slice the driver replays).
const { journey } = await (await p.request.get(`${API}/api/ui-state/journey?n=3`)).json();
console.log("journey:", journey.map((i) => `${i.kind}/${i.view}${i.eventId ? "@" + i.eventId.slice(0, 6) : ""}`).join("  →  "));

// 4. FORWARD replay — reset to a neutral view, then retrace. Should END at the
//    event (explore deep-link), reproducing where the journey ended.
console.log("=== FORWARD (retrace) ===");
await ctrl("open_view", { view: "overview" });
await p.waitForTimeout(1000);
for (const step of replay(journey, { direction: "forward", tempo: 2 })) {
  await ctrl(step.action, step.params);
  console.log(`  → ${step.action} ${JSON.stringify(step.params)}`);
  await p.waitForTimeout(700);
}
await p.waitForTimeout(1200);
console.log("forward end hash:", await p.evaluate(() => location.hash));
await p.screenshot({ path: `${S}/replay-forward.png` });

// 5. BACKWARD replay — rewind. Should END at the journey's START (overview).
console.log("=== BACKWARD (rewind) ===");
for (const step of replay(journey, { direction: "backward", tempo: 2 })) {
  await ctrl(step.action, step.params);
  console.log(`  ← ${step.action} ${JSON.stringify(step.params)}`);
  await p.waitForTimeout(700);
}
await p.waitForTimeout(1200);
console.log("backward end hash:", await p.evaluate(() => location.hash));
await p.screenshot({ path: `${S}/replay-backward.png` });

await b.close();
console.log("saved replay-captured / replay-forward / replay-backward");
