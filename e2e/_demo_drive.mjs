import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const OURS = "0375729d-4f5f-4043-bf1e-71a8ad37a187";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message.slice(0,160)));
// 1. sit on Overview, WS connected
await p.goto("http://127.0.0.1:5173/#/overview",{waitUntil:"networkidle"});
await p.waitForTimeout(3000);
console.log("BEFORE hash:", await p.evaluate(()=>location.hash));
await p.screenshot({path:`${S}/rev-drive-before.png`});
// 2. from OUTSIDE, an agent fires a view intent at OUR session's story
const res = await p.request.post("http://127.0.0.1:3002/api/control", {
  headers: {"content-type":"application/json"},
  data: { action:"open_view", params:{ route:`#/story/${OURS}` }, issuer:"claude · from our /loop" },
});
console.log("POST /api/control →", JSON.stringify(await res.json()));
// 3. the UI (a sink) reacts
await p.waitForTimeout(1500);
console.log("AFTER hash:", await p.evaluate(()=>location.hash));
const driven = await p.$eval('[data-testid="driven-by"]', e=>e.textContent.trim()).catch(()=>"(none)");
console.log("driven-by indicator:", driven);
await p.waitForTimeout(1500); // let story sentences load
await p.screenshot({path:`${S}/rev-drive-after.png`});
console.log("saved before/after");
await b.close();
