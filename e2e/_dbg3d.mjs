import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true,
  args: ["--no-sandbox","--disable-dev-shm-usage","--use-gl=angle","--use-angle=swiftshader","--enable-unsafe-swiftshader","--ignore-gpu-blocklist"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message.slice(0,200)));
p.on("console", m => console.log("CON["+m.type()+"]:", m.text().slice(0,180)));
await p.goto("http://127.0.0.1:5173/#/heatmap",{waitUntil:"networkidle"}); await p.waitForTimeout(1500);
const btns = await p.$$eval('button', bs => bs.filter(b=>/^(2D|3D)$/.test(b.textContent.trim())).map(b=>b.textContent.trim()));
console.log("toggle buttons found:", JSON.stringify(btns));
await p.getByRole("button",{name:"3D",exact:true}).click();
await p.waitForTimeout(3500);
const state = await p.evaluate(() => ({
  canvas: !!document.querySelector('canvas'),
  loading3d: document.body.innerText.includes('Loading 3D'),
  heightCaption: document.body.innerText.includes('height = sessions'),
  active: [...document.querySelectorAll('button')].filter(b=>/^(2D|3D)$/.test(b.textContent.trim())).map(b=>b.textContent.trim()+':'+(b.className.includes('7aa2f7')?'ON':'off')),
}));
console.log("state:", JSON.stringify(state));
await b.close();
