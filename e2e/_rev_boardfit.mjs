import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message));
// land directly on canvas (board is default) — do NOT press Fit
await p.goto("http://127.0.0.1:5173/#/canvas",{waitUntil:"networkidle"});
await p.waitForTimeout(3000); // let async sessions load + auto-fit fire
// measure: are all top-level group nodes within the viewport?
const info = await p.evaluate(() => {
  const svg = document.querySelector('svg');
  const vb = svg?.getBoundingClientRect();
  const groups = [...document.querySelectorAll('[data-kind="group"]')];
  let clipped = 0, total = groups.length;
  const vh = window.innerHeight, vw = window.innerWidth;
  for (const g of groups) {
    const r = g.getBoundingClientRect();
    if (r.top < 56 || r.bottom > vh || r.left < 0 || r.right > vw) clipped++;
  }
  return { total, clipped, svgTop: vb?.top };
});
console.log("top-level group nodes:", JSON.stringify(info));
await p.screenshot({path:`${S}/rev-board-autofit.png`});
console.log("saved");
await b.close();
