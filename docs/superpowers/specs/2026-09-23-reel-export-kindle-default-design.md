# Reel export with Kindle as the default format: design

**Date:** 2026-09-23 · **Status:** design under stated assumptions, not built.
Written from Max's ask: "plan to add a feature that enables export of reels, with
Kindle as the default format, and make it so the reels themselves are in the
Kindle style, visually."

**Depends on:** the reel export shipped in PR #112 (`ExportReelDialog.tsx`,
`export-collect.ts`, `export-sanitize.ts`, `export-scan.ts`, `reel-bundle.ts`;
spec `2026-08-22-reel-export-design.md`) and the Kindle reading edition on the
Codex branch `codex/kindle-reel-reports` (`rs/store/src/reel_report.rs`,
`ui/src/components/reels/ReelReports.tsx`, `ui/src/lib/reel-reports.ts`,
research note `docs/research/reel-reading-reports.md`). Aesthetic grounding:
`docs/research/kindle-reading-aesthetic.md`.

## The problem

Two export paths exist and they do not know about each other:

| | Self-contained HTML (#112, on master) | Kindle reading edition (Codex branch, unmerged) |
|---|---|---|
| Entry point | Export button in `ReelsView` opens `ExportReelDialog` | Separate "Kindle reading reports" section on the Reels page |
| Built where | Browser: collects rendered stages, sanitizes, scans, bakes | Server: `POST /api/reels/{id}/reports` renders from the reel JSON plus browser-captured images |
| Contract | `ReelBundle` JSON embedded in the HTML | `ReelReport` snapshot saved under `reels/reports/` |
| Share gate | Preview plus sensitive-content scan | None |
| Output | Interactive replay with captions and optional voice | Reflowable HTML for Send to Kindle, PNG figures, no script |

A reader who wants "the reel, on my Kindle" should not have to know which of the
two to press. And the Kindle edition should pass the same scan gate as the
interactive file: what leaves the machine is checked, whichever shape it takes.

## Decisions

1. **One Export dialog, a format picker, Kindle first.** `ExportReelDialog` gains
   a format row with two choices, **Kindle reading edition** (default) and
   **Interactive HTML**. Video stays listed as "coming" and disabled, because the
   reel-to-video design is its second consumer and this dialog is where it will
   land.
2. **Both formats are renderers of the same `ReelBundle`.** The bundle already
   holds opener, stops with narration, sanitized stage snapshots, figures, ink,
   and the scan receipt. The Kindle renderer consumes the bundle, not the raw
   reel, so the scan receipt travels with the edition. This retires the parallel
   `POST /api/reels/{id}/reports` collection path; the server keeps only the
   report *store* (save an edition, list editions, download one).
3. **The scan gate applies to every format.** Flagged content is shown before
   saving, whichever format is chosen. Same component, same receipt, embedded as a
   comment block in the Kindle HTML since KF8 strips script.
4. **Editions are durable snapshots.** Keep the Codex branch's rule: an edition is
   written once under `reels/reports/<id>.json` with its full HTML; later edits or
   deletion of the reel never change an earlier edition. Temp-file-and-rename.
5. **Delivery is a separate act.** v1 delivery is download. Email to a Kindle
   address is the BACKLOG entry "Kindle report email delivery" and stays out of
   this spec.
6. **The reel player adopts the reading aesthetic.** The Kindle edition should
   look like the reel, and the reel like the edition. Concretely the player gets:
   paper and ink tokens from the theme, a book serif for captions and narration,
   chrome that hides during playback with a thin footer (stop N of M on the left,
   percent on the right), and figure beats framed the way the edition frames them
   (image, title above, caption below). This is the first surface to adopt the
   aesthetic; the whole-UI question is the research note's, not this spec's.

## Kindle renderer contract

Input: `ReelBundle` (v1 schema in the #112 spec) plus a reading theme
`{ paper: "sepia" | "white" | "black" | "green", size: 1..14 }` chosen in the
dialog and stored as the edition's metadata.

Output: one HTML document that obeys the KF8 subset:

- No `<script>`, no `<canvas>`, no `<iframe>`, no `@import`, no external assets.
- Fonts by name only (`font-family: Bookerly, Georgia, serif`); no `@font-face`
  embedding in v1, since Kindle needs reader opt-in for embedded fonts anyway.
- Body text at `1em` with no forced color; secondary text in the `#666` to `#999`
  band; no black or white background on body. Paper color is a hint only, since
  the device applies its own theme.
- Images are `data:image/png` or `image/jpeg` only, longest edge at most 1200 px,
  each with `alt` from `visual.title` and a caption paragraph from the stop line.
  Diagram beats with `labels` render as an ordered list under the figure so the
  content survives image loss.
- Spotlight stops render the sanitized stage snapshot as prose (the text the
  viewer saw), with a small provenance line: session short id, event short id,
  timestamp. This is the citation the strategy thesis asks for.
- Structure: title page (title, author, created), opener as the first section,
  one section per stop with a running header, closer as the last section, then a
  colophon with the scan receipt and the bundle version.

Word count and warnings are computed at render time and stored on the edition,
matching the Codex branch's `ReportMeta`.

## Components and files

| Piece | Where | Change |
|---|---|---|
| Format picker and theme row | `ui/src/components/reels/ExportReelDialog.tsx` | Add; default Kindle |
| Kindle renderer | `ui/src/components/reels/export-kindle.ts` (new) | Pure: `bundle, theme -> { html, wordCount, warnings }` |
| Interactive renderer | `export-template.ts` | Unchanged |
| Edition store | `rs/store/src/reel_report.rs` (from Codex branch) | Keep store and metadata; drop server-side rendering; accept `{ html, meta }` |
| API | `rs/server/src/api.rs`, `router.rs` | `POST /api/reels/{id}/editions` saves a rendered edition; `GET /api/reel-editions`; `GET /api/reel-editions/{id}/download` |
| Editions list | `ReelReports.tsx` becomes `ReelEditions.tsx` | History with format badge, warnings, download |
| Player aesthetic | `reel-player.ts`, `ReelBeatStage.tsx`, `ReelsView.tsx` | Serif, hidden chrome, thin footer, figure framing |
| MCP | `rs/mcp/src/tools/reels.rs` | No new verb in v1; export remains a human act (per #112) |

Rename note: the Codex branch calls them "reports". "Edition" is the reading
word and matches the research note; the rename happens in this work, and the
Codex branch's on-disk directory stays `reels/reports/` for compatibility.

## Error handling

- A figure whose `imageHref` is not a raster data URL or cannot be embedded:
  keep the caption and labels, add a warning to the edition, never fail the
  export.
- A stage snapshot that fails sanitization: the stop renders as narration only
  with a warning, same as the interactive path.
- Scan flags: acknowledge-or-cancel, unchanged from #112.
- Store write failure: no edition entry is created; the dialog shows the error
  and offers download of the in-memory HTML so nothing is lost.

## Testing

BDD, red first, `describe("when X") / it("should Y")`:

- `export-kindle.test.ts`: given a bundle with one spotlight, one image, one
  diagram stop, the HTML has no script tags, one `<img>` with the right `alt`,
  an ordered list for the diagram labels, a provenance line per spotlight stop,
  and the scan receipt in a comment; word count equals the narration word count.
- `ExportReelDialog` component spec: Kindle is preselected; switching format
  swaps the preview; the scan gate blocks save in both formats.
- Rust `reel_report` tests: save is atomic, list is newest first, download sets
  the attachment header, an edition survives deleting its reel.
- Playwright: export a seeded reel as Kindle, download, open the file, assert
  the title and first caption are present and no script executed.
- A `--test` on `scripts/check_reel_reports.mjs` (Codex branch) if it is kept.

## Out of scope

- Email delivery to a Kindle address (BACKLOG).
- Video export (reel-to-video design).
- The whole-UI Kindle aesthetic (research note; decide after the player wears it).
- Agent-triggered export.
