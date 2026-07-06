import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage", "--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", (e) => console.log("PAGEERR:", e.message.slice(0, 160)));

// Scatter mode plots each session as a directly-clickable dot → reliable select.
await p.goto("http://127.0.0.1:5173/#/canvas", { waitUntil: "networkidle" });
await p.waitForTimeout(2000);
// switch to Scatter via the mode chip
await p.getByText("Scatter", { exact: false }).first().click().catch(() => {});
await p.waitForTimeout(1500);

async function asideWidth() {
  return p.evaluate(() => {
    const a = document.querySelector("aside");
    return a ? Math.round(a.getBoundingClientRect().width) : null;
  });
}

// Click dots until the detail aside opens.
let opened = false;
const dots = await p.$$("svg circle");
console.log("candidate dots:", dots.length);
for (const d of dots) {
  const box = await d.boundingBox();
  if (!box || box.width < 4) continue;
  await p.mouse.click(box.x + box.width / 2, box.y + box.height / 2);
  await p.waitForTimeout(400);
  if ((await asideWidth()) != null) { opened = true; break; }
}
console.log("panel opened:", opened, "| initial width:", await asideWidth());
if (!opened) { await b.close(); process.exit(1); }

const handle = p.locator('[aria-label="Resize panel"]');
const hb = await handle.boundingBox();

// WIDEN: drag the left-edge handle to the LEFT by 180px.
const w0 = await asideWidth();
await p.mouse.move(hb.x + hb.width / 2, hb.y + 300);
await p.mouse.down();
await p.mouse.move(hb.x - 180, hb.y + 300, { steps: 12 });
await p.mouse.up();
await p.waitForTimeout(300);
const wWide = await asideWidth();
console.log(`widen: ${w0} → ${wWide}`);
await p.screenshot({ path: `${S}/resize-wide.png` });

// NARROW: drag back to the RIGHT past the start.
const hb2 = await (p.locator('[aria-label="Resize panel"]')).boundingBox();
await p.mouse.move(hb2.x + hb2.width / 2, hb2.y + 300);
await p.mouse.down();
await p.mouse.move(hb2.x + 260, hb2.y + 300, { steps: 12 });
await p.mouse.up();
await p.waitForTimeout(300);
const wNarrow = await asideWidth();
console.log(`narrow: ${wWide} → ${wNarrow}`);
await p.screenshot({ path: `${S}/resize-narrow.png` });

// PERSIST: reload and confirm the width survives (reopen the panel).
const stored = await p.evaluate(() => window.localStorage.getItem("canvas.detail.width"));
console.log("localStorage canvas.detail.width:", stored);
console.log("clamped in [320,760]:", wWide <= 760 && wNarrow >= 320);

await b.close();
console.log("saved resize-wide / resize-narrow");
