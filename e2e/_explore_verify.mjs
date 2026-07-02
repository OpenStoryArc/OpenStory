import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const SESS = "2a0d4337-4d8a-4bfd-8d94-3c62c164568c";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox", "--disable-dev-shm-usage", "--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1500, height: 950 } });
p.on("pageerror", (e) => console.log("PAGEERR:", e.message.slice(0, 160)));
await p.goto(`http://127.0.0.1:5173/#/explore/${SESS}`, { waitUntil: "networkidle" });
await p.waitForTimeout(3500);
// what's the active tab + is the token report + story link present?
const activeTab = await p.evaluate(() => {
  const on = [...document.querySelectorAll("button")].find((b) => /Session|Events/.test(b.textContent) && b.className.includes("7aa2f7"));
  return on?.textContent?.trim() ?? "(none)";
});
const hasStory = await p.evaluate(() => [...document.querySelectorAll("button")].some((b) => b.textContent.trim() === "Story →"));
const hasTokens = await p.evaluate(() => document.body.textContent.includes("from cache") || document.body.textContent.includes("tokens total"));
console.log("active tab:", activeTab, "| Story→ link:", hasStory, "| token report:", hasTokens);
await p.screenshot({ path: `${S}/explore-redesign.png` });
console.log("saved explore-redesign.png");
await b.close();
