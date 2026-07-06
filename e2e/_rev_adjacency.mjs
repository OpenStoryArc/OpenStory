import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message.slice(0,140)));
await p.goto("http://127.0.0.1:5173/#/lab",{waitUntil:"networkidle"}); await p.waitForTimeout(2500);
// the tool-adjacency card should show "built" + an "open ▸" button
const built = await p.$('[data-open-viz="tool-adjacency-heatmap"]');
console.log("open-viz button present:", !!built);
await built.click();
await p.waitForSelector('[data-testid="lab-viewer"]',{timeout:5000});
// wait for the matrix to load (records fetch)
let cells=0; for(let i=0;i<12;i++){ await p.waitForTimeout(1000); cells=await p.$$eval('[data-testid="lab-viewer"] svg rect', r=>r.length).catch(()=>0); if(cells>10) break; }
console.log("heatmap cells rendered:", cells);
await p.screenshot({path:`${S}/rev-adjacency.png`});
console.log("saved");
await b.close();
