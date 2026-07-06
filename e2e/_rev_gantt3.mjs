import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message));
await p.goto("http://127.0.0.1:5173/#/canvas",{waitUntil:"networkidle"}); await p.waitForTimeout(1500);
await p.getByRole("button",{name:/Gantt/}).first().click(); await p.waitForTimeout(1800);
// the overview svg is height 68 now
const ov = await p.$('svg[height="68"]');
console.log("overview svg h=68 present:", !!ov);
const bars = await ov.$$eval('rect', rs => rs.filter(r=>+r.getAttribute('fill-opacity')===0.55).length);
console.log("density bars:", bars);
await p.screenshot({path:`${S}/rev-gantt-overview.png`});
console.log("saved");
await b.close();
