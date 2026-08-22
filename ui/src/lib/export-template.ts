/**
 * bakeReelHtml — the standalone offline HTML document. First renderer of a
 * ReelBundle; the video pipeline (docs/research/reel-to-video.md) is the
 * second. Pure: string in, string out — no DOM, no fetch, no imports of app
 * code (the output must run with zero external requests, so any rendering
 * logic used at view time is reimplemented as inline vanilla JS below, not
 * imported from `@/lib/draw` or the React components).
 */

import type { ReelBundle } from "@/lib/reel-bundle";

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function escapeAttr(s: string): string {
  return escapeHtml(s);
}

// Inline CSS: dark theme, system fonts, no webfonts, no external references.
const CSS = `
:root {
  --bg: #1a1b26;
  --text: #cdd6f9;
  --accent: #7aa2f7;
  --muted: #565f89;
  --panel: #20222f;
  --border: #33374a;
}
* { box-sizing: border-box; }
html, body {
  margin: 0;
  padding: 0;
  background: var(--bg);
  color: var(--text);
  font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
  height: 100%;
  overflow: hidden;
}
.reel-shell {
  position: relative;
  width: 100vw;
  height: 100vh;
  display: flex;
  flex-direction: column;
  background: var(--bg);
}
.reel-stage-wrap {
  position: relative;
  flex: 1;
  min-height: 0;
}
.reel-slide {
  position: absolute;
  inset: 0;
  display: none;
}
.reel-slide.is-active {
  display: block;
}
.reel-stage {
  position: absolute;
  inset: 0;
  display: flex;
  align-items: center;
  justify-content: center;
}
.reel-stage svg {
  width: 100%;
  height: 100%;
}
.reel-ink-overlay {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  pointer-events: none;
}
.reel-text-stage {
  max-width: 80vw;
  text-align: center;
  font-size: clamp(20px, 4vw, 40px);
  font-weight: 600;
  line-height: 1.4;
  padding: 0 2rem;
}
.reel-image-stage img {
  max-width: 92vw;
  max-height: 92vh;
  object-fit: contain;
  border-radius: 8px;
}
.reel-snapshot-frame {
  width: min(92vw, 1100px);
  max-height: 88vh;
  overflow: auto;
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 1.25rem;
}
.snap { color: var(--text); font-size: 15px; line-height: 1.5; }
.snap * { max-width: 100%; }
.snap pre, .snap code {
  font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  background: rgba(255, 255, 255, 0.04);
  border: 1px solid var(--border);
  border-radius: 6px;
}
.snap pre { padding: 0.75rem; overflow-x: auto; }
.snap code { padding: 0.1rem 0.3rem; }
.snap p { margin: 0.5rem 0; }
.snap a { color: var(--accent); }
.reel-caption-bar {
  min-height: 2.75rem;
  display: flex;
  align-items: center;
  justify-content: center;
  padding: 0.5rem 1.5rem;
  font-size: 16px;
  color: var(--text);
  background: rgba(0, 0, 0, 0.25);
  border-top: 1px solid var(--border);
  text-align: center;
}
.reel-controls {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 0.75rem;
  padding: 0.75rem 1rem 1.25rem;
  background: var(--bg);
}
.reel-btn {
  appearance: none;
  border: 1px solid var(--border);
  background: var(--panel);
  color: var(--text);
  border-radius: 999px;
  padding: 0.45rem 0.9rem;
  font-size: 15px;
  cursor: pointer;
  line-height: 1;
}
.reel-btn:hover { border-color: var(--accent); }
.reel-btn.is-on { border-color: var(--accent); color: var(--accent); }
.reel-dots {
  display: flex;
  align-items: center;
  gap: 0.4rem;
  margin: 0 0.5rem;
}
.reel-dot {
  width: 8px;
  height: 8px;
  border-radius: 999px;
  background: var(--muted);
  cursor: pointer;
  border: none;
  padding: 0;
}
.reel-dot.is-active { background: var(--accent); }
.reel-footer {
  padding: 0.5rem 1rem 0.75rem;
  text-align: center;
  font-size: 11px;
  color: var(--muted);
}
.reel-click-zone {
  position: absolute;
  inset: 0;
  cursor: pointer;
}
`;

// Inline vanilla JS: reads the embedded bundle JSON at load and renders it.
// No template literals / backticks used here — this string is a TS template
// literal itself, so any backtick or unescaped \${ inside would be parsed as
// TS interpolation. Plain string concatenation only.
const SCRIPT = `
(function () {
  "use strict";
  var dataEl = document.getElementById("reel-bundle");
  var bundle = JSON.parse(dataEl.textContent || dataEl.innerText || "{}");
  var slides = (bundle.reel && bundle.reel.slides) || [];
  var root = document.getElementById("reel-shell");
  var captionBar = document.getElementById("reel-caption-bar");
  var playBtn = document.getElementById("reel-play");
  var prevBtn = document.getElementById("reel-prev");
  var nextBtn = document.getElementById("reel-next");
  var speakBtn = document.getElementById("reel-speak");
  var dotsEl = document.getElementById("reel-dots");

  var current = 0;
  var playing = false;
  var timer = null;
  var speechOn = false;

  function pathToSvgD(points, closed) {
    if (!points || points.length === 0) return "";
    function to(p) {
      return (p.x * 1000).toFixed(1) + " " + (p.y * 1000).toFixed(1);
    }
    var d = "M " + to(points[0]);
    for (var i = 1; i < points.length; i++) d += " L " + to(points[i]);
    if (closed) d += " Z";
    return d;
  }

  function svgEl(tag) {
    return document.createElementNS("http://www.w3.org/2000/svg", tag);
  }

  function renderStrokesInto(svg, strokes) {
    for (var i = 0; i < strokes.length; i++) {
      var s = strokes[i];
      var el = null;
      if (s.type === "path") {
        el = svgEl("path");
        el.setAttribute("d", pathToSvgD(s.points, s.closed));
        el.setAttribute("stroke", s.stroke || "#94a3b8");
        el.setAttribute("stroke-width", String(s.strokeWidth || 2));
        el.setAttribute("fill", s.fill || "none");
        el.setAttribute("stroke-linecap", "round");
        el.setAttribute("stroke-linejoin", "round");
      } else if (s.type === "line") {
        el = svgEl("line");
        el.setAttribute("x1", String(s.x1 * 1000));
        el.setAttribute("y1", String(s.y1 * 1000));
        el.setAttribute("x2", String(s.x2 * 1000));
        el.setAttribute("y2", String(s.y2 * 1000));
        el.setAttribute("stroke", s.stroke || "#64748b");
        el.setAttribute("stroke-width", String(s.strokeWidth || 2));
        el.setAttribute("stroke-linecap", "round");
      } else if (s.type === "circle") {
        el = svgEl("circle");
        el.setAttribute("cx", String(s.cx * 1000));
        el.setAttribute("cy", String(s.cy * 1000));
        el.setAttribute("r", String(s.r * 1000));
        el.setAttribute("stroke", s.stroke || "#94a3b8");
        el.setAttribute("stroke-width", String(s.strokeWidth || 2));
        el.setAttribute("fill", s.fill || "none");
      } else if (s.type === "ellipse") {
        el = svgEl("ellipse");
        el.setAttribute("cx", String(s.cx * 1000));
        el.setAttribute("cy", String(s.cy * 1000));
        el.setAttribute("rx", String(s.rx * 1000));
        el.setAttribute("ry", String(s.ry * 1000));
        el.setAttribute("stroke", s.stroke || "#94a3b8");
        el.setAttribute("stroke-width", String(s.strokeWidth || 2));
        el.setAttribute("fill", s.fill || "none");
      } else if (s.type === "text") {
        el = svgEl("text");
        el.setAttribute("x", String(s.x * 1000));
        el.setAttribute("y", String(s.y * 1000));
        el.setAttribute("fill", s.fill || "#e2e8f0");
        el.setAttribute("font-size", String(s.fontSize || 16));
        el.setAttribute("text-anchor", "middle");
        el.textContent = s.text || "";
      } else if (s.type === "image") {
        el = svgEl("image");
        el.setAttribute("href", s.href || "");
        el.setAttribute("x", String(s.x * 1000));
        el.setAttribute("y", String(s.y * 1000));
        el.setAttribute("width", String(s.w * 1000));
        el.setAttribute("height", String(s.h * 1000));
        if (typeof s.opacity === "number") el.setAttribute("opacity", String(s.opacity));
      }
      if (el) svg.appendChild(el);
    }
  }

  function makeStrokeSvg(className) {
    var svg = svgEl("svg");
    svg.setAttribute("viewBox", "0 0 1000 1000");
    svg.setAttribute("preserveAspectRatio", "xMidYMid meet");
    svg.setAttribute("class", className);
    return svg;
  }

  function renderStage(container, slide) {
    container.innerHTML = "";
    var stage = slide.stage || { type: "text" };
    if (stage.type === "text") {
      var p = document.createElement("p");
      p.className = "reel-text-stage";
      p.textContent = slide.line || "";
      container.appendChild(p);
    } else if (stage.type === "strokes") {
      var svg = makeStrokeSvg("reel-stage-svg");
      renderStrokesInto(svg, stage.strokes || []);
      container.appendChild(svg);
    } else if (stage.type === "image") {
      var wrap = document.createElement("div");
      wrap.className = "reel-image-stage";
      var img = document.createElement("img");
      img.src = stage.dataUri || "";
      img.alt = slide.line || "";
      wrap.appendChild(img);
      container.appendChild(wrap);
    } else if (stage.type === "snapshot") {
      var frame = document.createElement("div");
      frame.className = "reel-snapshot-frame snap";
      frame.innerHTML = stage.html || "";
      container.appendChild(frame);
    }
  }

  function renderInk(container, slide) {
    container.innerHTML = "";
    var ink = slide.ink || [];
    if (!ink.length) return;
    var svg = makeStrokeSvg("reel-ink-svg");
    renderStrokesInto(svg, ink);
    container.appendChild(svg);
  }

  function wordsPacedMs(text) {
    var words = (text || "").trim().split(/\\s+/).filter(Boolean).length;
    return Math.max(3500, (words / 3) * 1000 + 2000);
  }

  function clearTimer() {
    if (timer) {
      clearTimeout(timer);
      timer = null;
    }
  }

  function stopSpeech() {
    if (window.speechSynthesis) window.speechSynthesis.cancel();
  }

  function speakSlide(slide) {
    if (!speechOn || !window.speechSynthesis) return;
    stopSpeech();
    var utter = new SpeechSynthesisUtterance(slide.line || "");
    window.speechSynthesis.speak(utter);
  }

  function updateDots() {
    var dots = dotsEl.querySelectorAll(".reel-dot");
    for (var i = 0; i < dots.length; i++) {
      if (i === current) dots[i].classList.add("is-active");
      else dots[i].classList.remove("is-active");
    }
  }

  function showSlide(index) {
    if (index < 0 || index >= slides.length) return;
    current = index;
    var sections = root.querySelectorAll(".reel-slide");
    for (var i = 0; i < sections.length; i++) {
      if (i === current) sections[i].classList.add("is-active");
      else sections[i].classList.remove("is-active");
    }
    var slide = slides[current];
    captionBar.textContent = slide.caption || "";
    captionBar.style.visibility = slide.caption ? "visible" : "hidden";
    updateDots();
    speakSlide(slide);
    scheduleNext();
  }

  function scheduleNext() {
    clearTimer();
    if (!playing) return;
    if (current >= slides.length - 1) {
      playing = false;
      updatePlayBtn();
      return;
    }
    var slide = slides[current];
    var ms = wordsPacedMs(slide.line);
    timer = setTimeout(function () {
      showSlide(current + 1);
    }, ms);
  }

  function updatePlayBtn() {
    playBtn.textContent = playing ? "\\u23F8 Pause" : "\\u25B6 Play";
    playBtn.classList.toggle("is-on", playing);
  }

  function togglePlay() {
    playing = !playing;
    updatePlayBtn();
    if (playing) scheduleNext();
    else clearTimer();
  }

  function goto(index) {
    clearTimer();
    showSlide(index);
  }

  function next() {
    goto(Math.min(current + 1, slides.length - 1));
  }

  function prev() {
    goto(Math.max(current - 1, 0));
  }

  function toggleSpeech() {
    speechOn = !speechOn;
    speakBtn.classList.toggle("is-on", speechOn);
    if (!speechOn) stopSpeech();
    else speakSlide(slides[current]);
  }

  playBtn.addEventListener("click", togglePlay);
  prevBtn.addEventListener("click", function () {
    playing = false;
    updatePlayBtn();
    prev();
  });
  nextBtn.addEventListener("click", function () {
    playing = false;
    updatePlayBtn();
    next();
  });
  speakBtn.addEventListener("click", toggleSpeech);

  var dotButtons = dotsEl.querySelectorAll(".reel-dot");
  for (var di = 0; di < dotButtons.length; di++) {
    (function (idx) {
      dotButtons[di].addEventListener("click", function () {
        playing = false;
        updatePlayBtn();
        goto(idx);
      });
    })(di);
  }

  var clickZones = root.querySelectorAll(".reel-click-zone");
  for (var ci = 0; ci < clickZones.length; ci++) {
    clickZones[ci].addEventListener("click", function () {
      playing = false;
      updatePlayBtn();
      next();
    });
  }

  document.addEventListener("keydown", function (e) {
    if (e.key === "ArrowRight") {
      playing = false;
      updatePlayBtn();
      next();
    } else if (e.key === "ArrowLeft") {
      playing = false;
      updatePlayBtn();
      prev();
    } else if (e.key === " ") {
      e.preventDefault();
      togglePlay();
    }
  });

  if (!window.speechSynthesis) {
    speakBtn.style.display = "none";
  }

  for (var si = 0; si < slides.length; si++) {
    var stageContainer = document.getElementById("reel-stage-" + si);
    var inkContainer = document.getElementById("reel-ink-" + si);
    renderStage(stageContainer, slides[si]);
    renderInk(inkContainer, slides[si]);
  }

  updatePlayBtn();
  showSlide(0);
})();
`;

function renderSlideSections(bundle: ReelBundle): string {
  const parts: string[] = [];
  bundle.reel.slides.forEach((slide, i) => {
    const active = i === 0 ? " is-active" : "";
    const caption = slide.caption ?? "";
    parts.push(
      `<section class="reel-slide${active}" data-slide="s${i}" data-caption="${escapeAttr(caption)}">` +
        `<div class="reel-stage" id="reel-stage-${i}"></div>` +
        `<svg class="reel-ink-overlay" id="reel-ink-${i}" viewBox="0 0 1000 1000" preserveAspectRatio="xMidYMid meet"></svg>` +
        `<div class="reel-click-zone"></div>` +
        `</section>`,
    );
  });
  return parts.join("\n");
}

function renderDots(bundle: ReelBundle): string {
  return bundle.reel.slides
    .map((_, i) => `<button type="button" class="reel-dot${i === 0 ? " is-active" : ""}" aria-label="Go to slide ${i + 1}"></button>`)
    .join("");
}

export function bakeReelHtml(bundle: ReelBundle): string {
  const json = JSON.stringify(bundle).replace(/<\/script>/g, "<\\/script>");
  const title = escapeHtml(bundle.reel.title || "Reel");
  const scanLabel =
    bundle.scan.findings === 0
      ? "clean"
      : `${bundle.scan.findings} findings ${bundle.scan.acknowledged ? "acknowledged" : "unacknowledged"}`;
  const footer = `Exported from OpenStory · ${escapeHtml(bundle.exportedAt)} · by ${escapeHtml(
    bundle.exportedBy,
  )} · scan: ${escapeHtml(scanLabel)}`;

  return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${title}</title>
<style>${CSS}</style>
</head>
<body>
<script type="application/json" id="reel-bundle">${json}</script>
<div class="reel-shell" id="reel-shell">
  <div class="reel-stage-wrap" id="reel-stage-wrap">
    ${renderSlideSections(bundle)}
  </div>
  <div class="reel-caption-bar" id="reel-caption-bar"></div>
  <div class="reel-controls">
    <button type="button" class="reel-btn" id="reel-prev">‹</button>
    <button type="button" class="reel-btn" id="reel-play">▶ Play</button>
    <button type="button" class="reel-btn" id="reel-next">›</button>
    <div class="reel-dots" id="reel-dots">${renderDots(bundle)}</div>
    <button type="button" class="reel-btn" id="reel-speak">🔊</button>
  </div>
  <div class="reel-footer">${footer}</div>
</div>
<script>${SCRIPT}</script>
</body>
</html>`;
}
