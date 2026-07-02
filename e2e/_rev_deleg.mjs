import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message.slice(0,140)));
await p.goto("http://127.0.0.1:5173/#/lab",{waitUntil:"networkidle"}); await p.waitForTimeout(2500);
await p.$('[data-open-viz="delegation-graph"]').then(x=>x.click());
await p.waitForSelector('[data-testid="lab-viewer"]',{timeout:5000});
let nodes=0,txt="";
for(let i=0;i<26;i++){ await p.waitForTimeout(1500); nodes=await p.$$eval('[data-testid="lab-viewer"] circle', c=>c.length).catch(()=>0); txt=await p.$eval('[data-testid="lab-viewer"]', e=>e.innerText.slice(0,120)).catch(()=>""); if(nodes>0) break; }
console.log("graph nodes:", nodes);
console.log("viewer text:", txt.replace(/\n/g,' '));
const links=await p.$$eval('[data-testid="lab-viewer"] line', l=>l.length).catch(()=>0);
console.log("links:", links);
await p.screenshot({path:`${S}/rev-deleg.png`});
console.log("saved");
await b.close();
