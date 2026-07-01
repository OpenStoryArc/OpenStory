import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message.slice(0,160)));
await p.goto("http://127.0.0.1:5173/#/story",{waitUntil:"networkidle"}); await p.waitForTimeout(2500);
// click the second session in the sidebar (first is the live one, huge)
const rows = await p.$$('aside button, [class*="Session"] button');
// click a session by its title text
try { await p.getByText("Would you be able to do what we did", {exact:false}).first().click(); } catch(e){ console.log("fallback click"); await rows[1]?.click(); }
await p.waitForTimeout(2500);
const lines = await p.$$eval('div.text-\\[11px\\].leading-snug', els => els.map(e=>e.textContent.trim()).slice(0,8));
console.log("sidebar story lines:", JSON.stringify(lines, null, 0).slice(0,500));
await p.screenshot({path:`${S}/rev-story-sidebar.png`});
console.log("saved");
await b.close();
