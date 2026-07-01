import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const IDS = "7677c7ce-a59e-4a27-8cf9-cada80b0dfc4,agent-a969c81ad1417b2f4,agent-af58edebc322d73ba".split(",");
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message.slice(0,140)));
await p.goto("http://127.0.0.1:5173/#/live",{waitUntil:"networkidle"}); await p.waitForTimeout(3000);
const res = await p.request.post("http://127.0.0.1:3002/api/control", { headers:{"content-type":"application/json"},
  data:{ action:"present", params:{ message:"Heads up — 3 sessions I think you should look at (openactor, no token telemetry).", sessionIds:IDS, route:"#/overview" }, issuer:"claude" }});
console.log("present →", JSON.stringify(await res.json()));
await p.waitForTimeout(1800);
const banner = await p.$eval('[data-testid="present-banner"]', e=>e.textContent.replace(/\s+/g,' ').trim()).catch(()=>"(none)");
console.log("banner text:", banner);
const chips = await p.$$eval('[data-present-session]', els=>els.length);
console.log("session chips:", chips, "· hash:", await p.evaluate(()=>location.hash));
await p.screenshot({path:`${S}/rev-present.png`});
console.log("saved");
await b.close();
