import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message));
await p.goto("http://127.0.0.1:5173/#/canvas",{waitUntil:"networkidle"}); await p.waitForTimeout(2000);
await p.getByRole("button",{name:"gantt",exact:true}).first().click(); await p.waitForTimeout(2000);
// group by project (many bands → tests empty-band collapse hardest)
try { await p.getByRole("button",{name:"project",exact:true}).first().click(); await p.waitForTimeout(1500); } catch(e){ console.log("no project chip", e.message.slice(0,50)); }
const bands1 = await p.$$eval('svg text', ts => ts.length);
await p.screenshot({path:`${S}/rev-gantt-default.png`}); console.log("gantt default (recent window)");
// narrow the brush to a tiny recent slice by re-dragging the overview brush far right
// overview strip is at bottom; drag a small window near the right edge
const box = await p.$('svg[height="46"]');
if (box) {
  const bb = await box.boundingBox();
  const y = bb.y + 30;
  await p.mouse.move(bb.x + bb.width - 120, y); await p.mouse.down(); await p.mouse.move(bb.x + bb.width - 30, y, {steps:10}); await p.mouse.up();
  await p.waitForTimeout(800);
}
await p.screenshot({path:`${S}/rev-gantt-narrow.png`}); console.log("gantt narrowed");
await b.close();
