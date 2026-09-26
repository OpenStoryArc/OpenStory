/**
 * export-kindle — the Kindle reading edition, the ReelBundle's second renderer.
 *
 * Pure: `bundle -> { html, wordCount, warnings }`. The output obeys the KF8
 * subset so it survives Send to Kindle: no script, canvas, iframe, or
 * external asset; images only as raster data URLs; everything else as prose.
 * Fonts are named, never embedded (Kindle needs reader opt-in anyway);
 * secondary text sits in the #666..#999 band and body has no forced
 * background, per the Kindle Publishing Guidelines. The scan receipt rides
 * in an HTML comment plus a colophon line, because KF8 strips script blocks.
 * Spec: docs/superpowers/specs/2026-09-23-reel-export-kindle-default-design.md
 */

import type { BundleSlide, BundleStage, ReelBundle } from "@/lib/reel-bundle";

export interface KindleEdition {
  readonly html: string;
  readonly wordCount: number;
  readonly warnings: readonly string[];
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/** Only inert, embedded raster images belong in a reading edition. */
export function isRasterDataUrl(href: string): boolean {
  const m = /^data:image\/(png|jpeg|gif);base64,([A-Za-z0-9+/=]+)$/.exec(href);
  return m !== null && m[2]!.length % 4 === 0 && m[2]!.length <= 8_000_000;
}

function paragraphs(text: string): string {
  return text
    .split(/\n{2,}/)
    .map((p) => p.trim())
    .filter(Boolean)
    .map((p) => `<p>${escapeHtml(p).replace(/\n/g, "<br>")}</p>`)
    .join("");
}

/** Snapshot stages hold sanitized stage HTML; the edition wants the words the
 *  viewer saw, as prose. Block boundaries become paragraph breaks. */
function snapshotToParagraphs(html: string): string {
  const text = html
    .replace(/<\/(p|div|li|h[1-6]|pre|tr|section|article|blockquote)>/gi, "\n\n")
    .replace(/<br\s*\/?>/gi, "\n")
    .replace(/<[^>]+>/g, "")
    .replace(/&nbsp;/g, " ")
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'");
  return paragraphs(text);
}

function headingFor(slide: BundleSlide, partNo: number): string {
  return slide.title?.trim() || `Part ${partNo}`;
}

function renderStage(stage: BundleStage, heading: string, warnings: string[]): string {
  switch (stage.type) {
    case "image":
      if (isRasterDataUrl(stage.dataUri)) {
        return `<figure><img src="${stage.dataUri}" alt="${escapeHtml(heading)}"><figcaption>${escapeHtml(heading)}</figcaption></figure>`;
      }
      warnings.push(`${heading}: image could not be embedded; caption retained.`);
      return `<p class="caption">Image unavailable in this edition.</p>`;
    case "snapshot":
      return `<blockquote class="stage">${snapshotToParagraphs(stage.html)}</blockquote>`;
    case "strokes": {
      const labels = stage.strokes.flatMap((s) => (s.type === "text" && s.text.trim() ? [s.text.trim()] : []));
      if (labels.length === 0) return "";
      return `<ol class="diagram">${labels.map((l) => `<li>${escapeHtml(l)}</li>`).join("")}</ol>`;
    }
    case "text":
      return "";
  }
}

export function bakeKindleHtml(bundle: ReelBundle): KindleEdition {
  const warnings: string[] = [];
  const title = escapeHtml(bundle.reel.title || "Reel");
  const slides = bundle.reel.slides;
  const opener = slides.filter((s) => s.role === "opener");
  const body = slides.filter((s) => s.role === "body");
  const closer = slides.filter((s) => s.role === "closer");

  const parts: string[] = [];
  parts.push(`<h1>${title}</h1>`);
  parts.push(`<p class="byline">${escapeHtml(bundle.reel.author)}</p>`);
  parts.push(`<p class="byline">OpenStory reading edition · ${escapeHtml(bundle.exportedAt.slice(0, 10))}</p>`);
  for (const s of opener) parts.push(`<section class="opener">${paragraphs(s.line)}</section>`);

  if (body.length > 0) {
    parts.push(`<h2>Contents</h2><ol class="contents">`);
    body.forEach((s, i) => parts.push(`<li><a href="#part-${i + 1}">${escapeHtml(headingFor(s, i + 1))}</a></li>`));
    parts.push(`</ol>`);
  }

  body.forEach((s, i) => {
    const heading = headingFor(s, i + 1);
    parts.push(`<section id="part-${i + 1}"><h2>${escapeHtml(heading)}</h2>`);
    parts.push(paragraphs(s.line));
    parts.push(renderStage(s.stage, heading, warnings));
    if (s.anchor) {
      parts.push(
        `<p class="source">Source: session ${escapeHtml(s.anchor.sessionId)} · event ${escapeHtml(s.anchor.eventId)}</p>`,
      );
    }
    parts.push(`</section>`);
  });

  for (const s of closer) parts.push(`<h2>Closing thoughts</h2><section class="closer">${paragraphs(s.line)}</section>`);

  const scanLabel =
    bundle.scan.findings === 0
      ? "clean"
      : `${bundle.scan.findings} findings ${bundle.scan.acknowledged ? "acknowledged" : "unacknowledged"}`;
  parts.push(
    `<p class="colophon">Exported from OpenStory · ${escapeHtml(bundle.exportedAt)} · by ${escapeHtml(bundle.exportedBy)} · scan: ${escapeHtml(scanLabel)}</p>`,
  );
  parts.push(`<!-- openstory-scan findings=${bundle.scan.findings} acknowledged=${bundle.scan.acknowledged} -->`);

  const wordCount = slides.reduce((n, s) => n + s.line.split(/\s+/).filter(Boolean).length, 0);

  const html = `<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>${title}</title><style>
body{font-family:Bookerly,Charter,Georgia,serif;line-height:1.6;margin:1em auto;padding:0 1em;max-width:38em}
h1{font-size:2em;line-height:1.15}h2{font-size:1.35em;line-height:1.3;page-break-after:avoid}
p{margin:0 0 1em}section{margin-top:2em}a{color:inherit}img{max-width:100%;height:auto}
figure{margin:1.5em 0;page-break-inside:avoid}figcaption,.caption,.byline,.colophon{font-size:.85em;color:#666}
.stage{margin:1em 0;padding:0 0 0 1em;border-left:2px solid #999;color:#333}
.source{font-size:.7em;overflow-wrap:anywhere;color:#777}.diagram li{border:1px solid #999;padding:.6em;margin:.5em 0;page-break-inside:avoid}
</style></head><body>${parts.join("\n")}</body></html>`;

  return { html, wordCount, warnings };
}
