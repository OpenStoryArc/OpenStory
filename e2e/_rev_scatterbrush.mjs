import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message));
p.on("console", m => { if (m.type()==="error") console.log("CONSOLE-ERR:", m.text().slice(0,120)); });
await p.goto("http://127.0.0.1:5173/#/canvas",{waitUntil:"networkidle"}); await p.waitForTimeout(2000);
await p.getByRole("button",{name:"scatter",exact:true}).first().click(); await p.waitForTimeout(2500);
await p.screenshot({path:`${S}/rev-scatter-base.png`}); console.log("scatter base");
// enter select mode
await p.getByRole("button",{name:"Select",exact:true}).first().click(); await p.waitForTimeout(400);
console.log("brush layer present:", !!(await p.$('[data-testid="scatter-brush"]')));
// drag a marquee across the cloud
await p.mouse.move(450, 250); await p.mouse.down(); await p.mouse.move(1000, 650, {steps: 20}); await p.mouse.up();
await p.waitForTimeout(600);
const listItems = await p.$$('div.absolute.bottom-3 button');
console.log("brushed list items:", listItems.length);
await p.screenshot({path:`${S}/rev-scatter-brush.png`}); console.log("scatter brushed");
await b.close();
