import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message));
await p.goto("http://127.0.0.1:5173/#/canvas",{waitUntil:"networkidle"}); await p.waitForTimeout(1500);
await p.getByRole("button",{name:"sunburst",exact:true}).first().click(); await p.waitForTimeout(2000);
// count text labels inside the sunburst svg (excluding the center label)
const labels = await p.$$eval('svg g text', ts => ts.map(t=>t.textContent).filter(x=>x&&x.trim()));
console.log("sunburst inline labels:", labels.length, "→", labels.slice(0,12).join(" | "));
await p.screenshot({path:`${S}/rev-sunburst-labels.png`});
// also treemap to confirm still fine
await p.getByRole("button",{name:"treemap",exact:true}).first().click(); await p.waitForTimeout(1500);
await p.screenshot({path:`${S}/rev-treemap-labels.png`});
console.log("saved");
await b.close();
