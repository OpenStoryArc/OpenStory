# Reel export — design spec

**Date:** 2026-08-22 · **Branch:** `feat/reel-export` (off `master`, post-#110)
**Origin:** "can you make this something I can send to Katie as a video? Without her
having to pull this branch?" — the Grok session ask, refined in brainstorming.

## Decision summary (from brainstorming with Max)

- **Shape:** Option 4 — self-contained interactive **HTML file first**, with the
  architecture pointed at the video export (Option 1). The load-bearing contract is
  the **ReelBundle JSON schema**; the HTML template is its first renderer, the
  future video pipeline (docs/research/reel-to-video.md) is its second.
- **Share gate:** **Preview + scan.** Export opens a preview of exactly what the
  file will contain; a sensitive-content scan runs over the bundle; flagged items
  are shown before saving. No silent leaks.
- **Narration:** **Captions + optional voice.** Exported reel autoplays silently
  with caption-paced timing; a 🔊 toggle enables the recipient's browser speech
  synthesis.
- **Spotlight fidelity:** **snapshot, not data.** Spotlight stops are captured as
  sanitized HTML of the actually-rendered stage. What leaves the machine is exactly
  what was on screen — never raw event payloads (a truncated render must not smuggle
  its full 90KB payload).
- **Bundle is the embedded metadata.** The bundle JSON rides inside the artifact
  (`<script type="application/json" id="reel-bundle">`), making the file
  machine-readable and future-importable. The video export will embed the same
  bundle as MP4 metadata + per-beat chapter provenance (known caveat: platforms
  that transcode strip metadata — on-screen citations stay burned into pixels).
- **Beat ink ships by default** — slide annotations are part of the telling; the
  preview shows them so nothing rides along unseen.

## Soul constraints

- Export reads history and authored ink; it never mutates either. No new CloudEvents.
- `exportedBy` uses the person display_name only — never email (committed-artifact
  rule: no personal emails in files that leave the machine).
- The scan receipt is embedded in the bundle: an export always says whether its
  gate was clean or consciously overridden.

## ReelBundle schema (v1) — approved by Max

```ts
interface ReelBundle {
  v: 1;
  kind: "openstory.reel-bundle";
  exportedAt: string;            // ISO
  exportedBy: string;            // display_name, no email
  reel: {
    id: string; title: string; author: string; created: string;
    slides: BundleSlide[];       // unified, opener/closer folded in (pre-resolved)
  };
  scan: { v: 1; findings: number; acknowledged: boolean };
}

interface BundleSlide {
  id: string;
  kind: "title" | "diagram" | "image" | "spotlight";
  role: "opener" | "body" | "closer";
  line: string;
  caption: string | null;        // captionFor(slide)
  anchor?: { sessionId: string; eventId: string };   // spotlight provenance
  stage:
    | { type: "text" }
    | { type: "strokes"; strokes: DrawStroke[] }     // unit-space, pen language
    | { type: "image"; dataUri: string }
    | { type: "snapshot"; html: string };            // sanitized DOM snapshot
  ink?: DrawStroke[];            // beat ink for this slide
}
```

Slides are pre-resolved (`normalizeReelToSlides` runs at export time, not in the
template). Every stage is self-contained: no URLs, no fetches at view time.

## Components

All UI-side; the server is untouched. New files, one responsibility each:

1. **`ui/src/lib/reel-bundle.ts`** — pure. Types above + `buildBundle(reel,
   stages, ink, meta): ReelBundle` where `stages` and `ink` are supplied by the
   collector (pure function of its inputs; no I/O). Also `bundleText(bundle):
   {slideId, field, text}[]` — the flattened text view the scanner consumes
   (includes snapshot HTML text content).
2. **`ui/src/lib/export-sanitize.ts`** — pure. `sanitizeSnapshotHtml(html):
   string` — strips `<script>`, `<iframe>`, `on*` attributes, `javascript:` URLs,
   external resource references (img src → dropped unless data:); keeps class
   names and inline styles. Allowlist-based element/attribute walk over a
   DOMParser tree, not regex.
3. **`ui/src/lib/export-scan.ts`** — pure. `scanBundle(bundle): Finding[]` with
   `Finding {slideId, family, excerpt}`. Pattern families: AWS/access keys,
   generic `api_key|token|secret|password =`-style assignments, PEM blocks,
   bearer tokens, email addresses, absolute home paths (`/Users/<name>`, from
   snapshot text). Excerpts are trimmed to ≤80 chars.
4. **`ui/src/lib/export-template.ts`** — pure. `bakeReelHtml(bundle): string` —
   the complete standalone HTML document: embedded bundle JSON, inline CSS
   (dark, self-contained; includes the export-spotlight style subset), inline
   vanilla-JS player: slide render (title text / stroke SVG / image / snapshot),
   caption bar (respecting `caption: null`), progress dots, click + arrow
   navigation, caption-paced autoplay with Play/Pause, 🔊 speech-synthesis
   toggle, ink overlay SVG per slide. No external requests of any kind.
5. **`ui/src/lib/export-collect.ts`** — the one side-effectful module. Gathers
   what `buildBundle` needs from the running app: fetch reel (existing
   `reels-api`), render each spotlight stage offscreen via the real
   `EventSpotlight` into a detached container and capture `innerHTML`
   (sanitized via #2), inline image stops to data URIs, read beat ink from
   `streams/reel-annotate`, read display_name from `/api/config`-equivalent or
   fall back to `"openstory"`.
6. **`ui/src/components/reels/ExportReelDialog.tsx`** — the flow: triggered from
   a new "Export" affordance on the reel row + the player idle screen. Shows
   the preview by rendering the baked HTML in a sandboxed `<iframe srcdoc>`
   (the preview IS the artifact — no separate preview renderer), the scan
   findings list (grouped by slide, family-labelled), and:
   - findings = 0 → primary button "Save reel file"
   - findings > 0 → findings shown, button reads "Export anyway" (sets
     `scan.acknowledged: true`, re-bakes with the receipt) beside "Cancel".
   Download via Blob + `a.download` = `<reel-title-slug>.reel.html`.
7. **Docs:** BACKLOG entry for the video renderer as second bundle consumer
   (pointing at docs/research/reel-to-video.md and this spec); note in
   `docs/agent-in-ui.md` that export is a human affordance (no control verb in
   v1).

## Error handling

- A spotlight whose event no longer resolves renders the player's own missing
  state; the snapshot captures that honestly (no fabrication).
- Snapshot capture failure for a slide → export proceeds with a
  `{type:"text"}` stage carrying the line, and the preview marks the slide
  "snapshot unavailable" (visible, not silent).
- Image fetch failure → same downgrade, marked in preview.
- localStorage ink read failures → `ink` omitted (never blocks export).

## Testing (TDD throughout; red-green evidence per task)

- `reel-bundle`: buildBundle folds opener/closer, preserves anchors, carries
  ink; bundleText flattens snapshot text; schema round-trips JSON.
- `export-sanitize`: strips script/iframe/on*/javascript: and external img;
  keeps text, classes, data: URIs; idempotent.
- `export-scan`: one spec per pattern family (positive + negative); excerpt
  trimming; clean bundle → [].
- `export-template`: baked doc contains exactly one parseable
  `#reel-bundle` JSON equal to input; contains no `<script src`, no
  `http(s)://` references; caption suppressed for title slides; per-slide
  sections present.
- `ExportReelDialog` component spec (ui/tests/components/): findings gate the
  button label; acknowledged receipt lands in the re-baked JSON.
- Out of scope for v1 tests: opening the baked file in a real browser (manual
  walkthrough step at the end, Chrome-driven).

## Out of scope (recorded, not built)

- Video renderer (second consumer; reel-to-video.md is its design).
- Hosted links / publish-to-URL.
- Import of a `.reel.html` back into OpenStory (schema is versioned for it).
- Redaction-in-place editing of flagged content (v1 is acknowledge-or-cancel).
- Agent-triggered export (control verb) — human act in v1.
