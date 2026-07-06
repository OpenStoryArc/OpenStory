import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message));
await p.goto("http://127.0.0.1:5173/#/canvas",{waitUntil:"networkidle"}); await p.waitForTimeout(1500);
await p.getByRole("button",{name:"flow",exact:true}).first().click();
for (let i=0;i<12;i++){ await p.waitForTimeout(1000); const c=await p.$$eval('div.flex.items-center span', els=>els.map(e=>e.textContent).join(" ")); if(!/loading|sampling/.test(c)) break; }
// baseline: distinct ribbon opacities
const before = await p.$$eval('svg path', ps => ps.map(p=>+(p.getAttribute('fill-opacity')||0)));
console.log("ribbons:", before.length, "distinct-op:", [...new Set(before.map(x=>x.toFixed(2)))].join(","));
// hover the "Read" from-node text (left side)
const readNode = await p.getByText("Read", {exact:true}).first();
await readNode.hover();
await p.waitForTimeout(500);
const after = await p.$$eval('svg path', ps => ps.map(p=>+(p.getAttribute('fill-opacity')||0)));
const dimmed = after.filter(o=>o<=0.06).length, lit = after.filter(o=>o>=0.6).length;
console.log("on hover → dimmed(<=0.06):", dimmed, "lit(>=0.6):", lit);
await p.screenshot({path:`${S}/rev-flow-hover.png`});
console.log("saved");
await b.close();
