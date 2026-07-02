import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--use-gl=angle","--use-angle=swiftshader","--enable-unsafe-swiftshader","--ignore-gpu-blocklist"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message.slice(0,140)));
const drive = async (target,value) => { const r=await p.request.post("http://127.0.0.1:3002/api/control",{headers:{"content-type":"application/json"},data:{action:"toggle",params:{target,value},issuer:"claude"}}); return (await r.json()); };

// Heatmap: drive 2D → 3D and weeks
await p.goto("http://127.0.0.1:5173/#/heatmap",{waitUntil:"networkidle"}); await p.waitForTimeout(2500);
console.log("weeks→52:", JSON.stringify(await drive("heatmap.weeks","52"))); await p.waitForTimeout(1200);
console.log("dim→3d:", JSON.stringify(await drive("heatmap.dim","3d")));
await p.waitForSelector('canvas',{timeout:8000}).catch(()=>{});
await p.waitForTimeout(3500);
console.log("heatmap canvas present:", !!(await p.$('canvas')));
await p.screenshot({path:`${S}/rev-toggle-heatmap.png`});

// Story: drive sort
await p.goto("http://127.0.0.1:5173/#/story",{waitUntil:"networkidle"}); await p.waitForTimeout(2500);
console.log("story.sort→tokens:", JSON.stringify(await drive("story.sort","tokens"))); await p.waitForTimeout(1500);
const activeSort = await p.$$eval('button', bs => bs.filter(b=>b.className.includes("7aa2f7")&&/most tokens/i.test(b.textContent)).length);
console.log("story 'Most tokens' active:", activeSort);
await p.screenshot({path:`${S}/rev-toggle-story.png`});
console.log("saved");
await b.close();
