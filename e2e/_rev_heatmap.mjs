import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message));
p.on("console", m => { if (m.type()==="error") console.log("CONSOLE-ERR:", m.text().slice(0,120)); });
await p.goto("http://127.0.0.1:5173/#/heatmap",{waitUntil:"networkidle"}); await p.waitForTimeout(2500);
const cells = await p.$$eval('[data-heat-cell]', els => els.length);
const lit = await p.$$eval('[data-heat-cell]', els => els.filter(e=>e.getAttribute('fill')!=='#1b1f2a').length);
console.log("heat cells:", cells, "· lit:", lit);
await p.screenshot({path:`${S}/rev-heatmap-2d.png`});
// filter by an agent chip + zoom to 52w
try { await p.getByRole("button",{name:/^claude-code/}).first().click(); await p.waitForTimeout(600); } catch(e){ console.log("no agent chip"); }
await p.getByRole("button",{name:"52w",exact:true}).click(); await p.waitForTimeout(700);
const lit2 = await p.$$eval('[data-heat-cell]', els => els.filter(e=>e.getAttribute('fill')!=='#1b1f2a').length);
console.log("after claude-code filter + 52w → lit:", lit2);
await p.screenshot({path:`${S}/rev-heatmap-2d-filtered.png`});
console.log("saved");
await b.close();
