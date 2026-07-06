import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message));

// Explore sidebar — titles should be readable, not raw XML / bare hex
await p.goto("http://127.0.0.1:5173/#/explore",{waitUntil:"networkidle"}); await p.waitForTimeout(2500);
await p.screenshot({path:`${S}/rev-titles-explore.png`}); console.log("explore titles");
const rawXml = await p.$$eval('body', els => (els[0].innerText.match(/<command-message>/g)||[]).length);
console.log("raw <command-message> occurrences on Explore:", rawXml);

// ⌘K palette — should list Canvas + Ask
await p.keyboard.press("Meta+k"); await p.waitForTimeout(600);
const navTitles = await p.$$eval('[data-palette-item^="tab-"] .text-\\[13px\\], [data-palette-item^="tab-"]', els => els.map(e => e.textContent?.trim()).filter(Boolean));
console.log("palette tab items:", JSON.stringify(navTitles));
await p.screenshot({path:`${S}/rev-titles-palette.png`}); console.log("palette");
await b.close();
