#!/usr/bin/env node
/**
 * broadcast_drop_probe.mjs — the missing NFR instrument: measures whether the
 * WebSocket broadcast path DROPS frames under a burst (the "ZERO dropped
 * broadcasts" NFR the metronome couldn't see).
 *
 * How a drop happens: every browser subscribes to the server's tokio broadcast
 * channel (capacity 256). If a client can't drain fast enough it lags past the
 * buffer and the server skips frames (`RecvError::Lagged(n)`). This probe is a
 * real WS client (Node's built-in WebSocket): it connects, then fires a burst of
 * N `control` messages and counts how many `kind:"control"` frames it actually
 * receives back. sent − received = dropped.
 *
 * Sovereignty: drives only ui.* (POST /api/control); never events.*.
 *
 * Usage: node scripts/broadcast_drop_probe.mjs [--base http://localhost:3002]
 *        [--bursts 64,256,512,1024] [--settle 2500]
 */

const argv = process.argv.slice(2);
const flag = (n, d) => { const i = argv.indexOf(`--${n}`); return i >= 0 && i + 1 < argv.length ? argv[i + 1] : d; };
const BASE = flag("base", "http://localhost:3002");
const WS_URL = BASE.replace(/^http/, "ws") + "/ws";
const BURSTS = flag("bursts", "64,256,512,1024").split(",").map(Number);
const SETTLE = Number(flag("settle", "2500"));

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// live count of control frames per issuer tag, updated by the WS onmessage.
const received = new Map();

function connect() {
  return new Promise((resolve, reject) => {
    const ws = new WebSocket(WS_URL);
    ws.addEventListener("open", () => resolve(ws));
    ws.addEventListener("error", (e) => reject(e.message ?? e));
    ws.addEventListener("message", (ev) => {
      let m;
      try { m = JSON.parse(ev.data); } catch { return; }
      if (m?.kind !== "control") return;
      const tag = m.issuer ?? "";
      received.set(tag, (received.get(tag) ?? 0) + 1);
    });
  });
}

async function fireBurst(n, tag) {
  received.set(tag, 0);
  const posts = [];
  for (let i = 0; i < n; i++) {
    posts.push(
      fetch(`${BASE}/api/control`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ action: "toggle", params: { target: "canvas.mode", value: "board", seq: i }, issuer: tag }),
      }).catch(() => {}),
    );
  }
  await Promise.all(posts);
  await sleep(SETTLE); // let the WS drain
  return received.get(tag) ?? 0;
}

async function main() {
  console.log(`\n📡 broadcast-drop probe — 1 WS client, channel cap 256, ${WS_URL}\n`);
  let ws;
  try { ws = await connect(); } catch (e) { console.error("WS connect failed:", e); process.exit(1); }
  await sleep(400); // absorb initial_state

  const rows = [];
  for (const n of BURSTS) {
    const tag = `drop-probe-${n}-${rows.length}`;
    const got = await fireBurst(n, tag);
    const dropped = Math.max(0, n - got);
    const pct = ((dropped / n) * 100).toFixed(1);
    rows.push({ n, got, dropped, pct });
    console.log(`burst ${String(n).padStart(5)} → sent ${n}  received ${got}  dropped ${dropped} (${pct}%)`);
  }
  ws.close();

  const worst = rows.reduce((a, b) => (b.dropped > a.dropped ? b : a), rows[0]);
  console.log(`\nVerdict: ${worst.dropped === 0
    ? "ZERO dropped broadcasts across all bursts ✅ — the NFR holds."
    : `first drops at burst ${rows.find((r) => r.dropped > 0).n} (a single slow client lagging past the 256 buffer) ❌`}`);
  console.log("Note: one WS client draining as fast as Node can. A real browser drains slower under render load — the true ceiling is lower.\n");
}

main().catch((e) => { console.error(e); process.exit(1); });
