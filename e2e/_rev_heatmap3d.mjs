import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true,
  args: ["--no-sandbox","--disable-dev-shm-usage","--use-gl=angle","--use-angle=swiftshader","--enable-unsafe-swiftshader","--ignore-gpu-blocklist"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message.slice(0,160)));
await p.goto("http://127.0.0.1:5173/#/heatmap",{waitUntil:"networkidle"}); await p.waitForTimeout(1800);
await p.getByRole("button",{name:"3D",exact:true}).click();
await p.waitForSelector('canvas',{timeout:8000});
await p.waitForTimeout(900); // catch the rise mid-animation
await p.screenshot({path:`${S}/rev-heatmap-3d-rising.png`});
await p.waitForTimeout(3000); // settled
await p.screenshot({path:`${S}/rev-heatmap-3d.png`});
console.log("canvas:", !!(await p.$('canvas')), "· saved rising + settled");
await b.close();
