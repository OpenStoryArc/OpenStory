#!/usr/bin/env node
/**
 * Print a slide-deck HTML file to PDF with Playwright's Chromium.
 *
 * One slide per page, landscape 16:9, backgrounds kept. Used by
 * scripts/reel_slides.py, which writes the HTML; this only prints it.
 *
 *   node scripts/html_to_pdf.mjs deck.html deck.pdf
 *
 * Runs from the repo's e2e/ dependencies (Playwright is already installed
 * there for the E2E suite), so it needs no new packages.
 */
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";
import { resolve } from "node:path";

const [, , htmlPath, pdfPath] = process.argv;
if (!htmlPath || !pdfPath) {
  console.error("usage: node scripts/html_to_pdf.mjs <deck.html> <deck.pdf>");
  process.exit(2);
}

const require = createRequire(resolve(process.cwd(), "e2e/package.json"));
const { chromium } = require("playwright");

const browser = await chromium.launch();
try {
  const page = await browser.newPage({ viewport: { width: 1600, height: 900 } });
  await page.goto(pathToFileURL(resolve(htmlPath)).href, { waitUntil: "load" });
  await page.emulateMedia({ media: "print" });
  await page.pdf({
    path: resolve(pdfPath),
    width: "1600px",
    height: "900px",
    printBackground: true,
    preferCSSPageSize: true,
    margin: { top: 0, right: 0, bottom: 0, left: 0 },
  });
  console.log(`wrote ${pdfPath}`);
} finally {
  await browser.close();
}
