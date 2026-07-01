import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message));
await p.goto("http://127.0.0.1:5173/#/canvas",{waitUntil:"networkidle"}); await p.waitForTimeout(1500);
await p.getByRole("button",{name:"scatter",exact:true}).first().click(); await p.waitForTimeout(2000);
// measure spread of cx among the leftmost (low-event) points
const xs = await p.$$eval('circle[data-scatter-point]', cs => cs.map(c=>+c.getAttribute('cx')));
const left = xs.filter(x=>x<140).sort((a,b)=>a-b);
console.log("total points:", xs.length, "· near-axis(<140px):", left.length, "· distinct cx:", new Set(left.map(x=>x.toFixed(1))).size);
await p.screenshot({path:`${S}/rev-scatter-jitter.png`});
console.log("saved");
await b.close();
