import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message));
await p.goto("http://127.0.0.1:5173/#/canvas",{waitUntil:"networkidle"}); await p.waitForTimeout(1500);
await p.getByRole("button",{name:"flow",exact:true}).first().click();
// wait up to ~11s for it to resolve past loading
let caption = "";
for (let i=0;i<12;i++){ await p.waitForTimeout(1000); caption = (await p.$$eval('div.flex.items-center span', els=>els.map(e=>e.textContent).join(" | "))).slice(0,300); if(!/loading|sampling/.test(caption)) break; }
console.log("FINAL caption:", caption);
await p.screenshot({path:`${S}/rev-flow-claude-fixed.png`});
console.log("saved");
await b.close();
