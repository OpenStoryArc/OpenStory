import { chromium } from "playwright";
const S = "/tmp/claude-1000/-home-max-projects-OpenStory/0375729d-4f5f-4043-bf1e-71a8ad37a187/scratchpad";
const OURS = "0375729d-4f5f-4043-bf1e-71a8ad37a187";
const b = await chromium.launch({ executablePath: `${S}/chrome-linux/chrome`, headless: true, args: ["--no-sandbox","--disable-dev-shm-usage","--disable-gpu"] });
const p = await b.newPage({ viewport: { width: 1400, height: 900 } });
p.on("pageerror", e => console.log("PAGEERR:", e.message.slice(0,140)));
// the human navigates around — each hop emits an interaction event
for (const route of ["#/overview","#/heatmap","#/canvas",`#/story/${OURS}`]) {
  await p.goto(`http://127.0.0.1:5173/${route}`, { waitUntil: "networkidle" });
  await p.waitForTimeout(1200);
}
await p.waitForTimeout(800);
// what an agent reads to know where you are:
const uiState = await (await p.request.get("http://127.0.0.1:3002/api/ui-state")).json();
console.log("GET /api/ui-state →", JSON.stringify(uiState));
// prove it's a real session in the store, with finer subtypes:
const recs = await (await p.request.get(`http://127.0.0.1:3002/api/sessions/openstory-ui/records`)).json().catch(()=>({}));
const arr = Array.isArray(recs) ? recs : (recs.records || []);
console.log("viewing-session records in store:", arr.length);
await b.close();
