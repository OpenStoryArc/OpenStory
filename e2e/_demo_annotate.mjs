import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const OURS = "0375729d-4f5f-4043-bf1e-71a8ad37a187";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message.slice(0,140)));
await p.goto("http://127.0.0.1:5173/#/overview",{waitUntil:"networkidle"}); await p.waitForTimeout(3000);
// an agent pins a durable note to our session
const res = await p.request.post("http://127.0.0.1:3002/api/annotations",{headers:{"content-type":"application/json"},
  data:{ session_id: OURS, body: "This is the session where we built the control seam + annotations. Start here.", issuer: "claude" }});
console.log("POST /api/annotations →", JSON.stringify(await res.json()).slice(0,120));
await p.waitForTimeout(1500);
const overlay = await p.$eval('[data-testid="annotations-overlay"]', e=>e.textContent.replace(/\s+/g,' ').trim()).catch(()=>"(none)");
console.log("overlay:", overlay);
await p.screenshot({path:`${S}/rev-annotate.png`});
// prove durability: reload → note persists (from JSONL via GET)
await p.reload({waitUntil:"networkidle"}); await p.waitForTimeout(2500);
const afterReload = await p.$('[data-testid="annotations-overlay"]');
console.log("persists after reload:", !!afterReload);
console.log("saved");
await b.close();
