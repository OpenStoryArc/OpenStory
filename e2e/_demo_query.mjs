import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message.slice(0,140)));
await p.goto("http://127.0.0.1:5173/#/live",{waitUntil:"networkidle"}); await p.waitForTimeout(3000);
// an agent narrows the fleet to pi-mono sessions, sorted by tokens
const res = await p.request.post("http://127.0.0.1:3002/api/control",{headers:{"content-type":"application/json"},
  data:{ action:"query", params:{ agent:"pi-mono", sort:"tokens" }, issuer:"claude" }});
console.log("query →", JSON.stringify(await res.json()));
await p.waitForTimeout(1800);
console.log("hash:", await p.evaluate(()=>location.hash));
await p.screenshot({path:`${S}/rev-query.png`});
console.log("saved");
await b.close();
