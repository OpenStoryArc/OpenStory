import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message.slice(0,140)));
const drive = async (target,value) => { const r=await p.request.post("http://127.0.0.1:3002/api/control",{headers:{"content-type":"application/json"},data:{action:"toggle",params:{target,value},issuer:"claude"}}); return (await r.json()); };
await p.goto("http://127.0.0.1:5173/#/canvas",{waitUntil:"networkidle"}); await p.waitForTimeout(2000);
console.log("mode→sunburst:", JSON.stringify(await drive("canvas.mode","sunburst"))); await p.waitForTimeout(1500);
console.log("drill→OpenStory:", JSON.stringify(await drive("canvas.drill","OpenStory"))); await p.waitForTimeout(1500);
// read the breadcrumb to prove the shape zoomed in
const crumb = await p.$$eval('button', bs => bs.map(b=>b.textContent.trim()).filter(t=>/^(all|OpenStory|max|maxglassie|unknown)$/i.test(t))).catch(()=>[]);
console.log("breadcrumb:", JSON.stringify(crumb));
await p.screenshot({path:`${S}/rev-drill.png`});
console.log("saved");
await b.close();
