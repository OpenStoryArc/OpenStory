# The reading system

**Status:** proposal, 2026-09-23. The Kindle reading aesthetic abstracted into
rules OpenStory can apply. Companion files: `reading-system-mockup.html` (the
rules rendered) and `openstory-mark.svg` (the mark).

## Where the research lives

`docs/research/kindle-reading-aesthetic.md` is the research note, with sources.
Kindle publishes no design language; what exists is the device, the KDP
publishing rules, and the Kindle Create themes. From those the note drew eight
decisions a designer would copy, and one lesson from the critiques: the reading
page is admired, the navigation is not. It also found the ground already
prepared. The light theme went sepia in July (`b7fb5ca`), the header has a
stepped S/M/L/XL text control, `scripts/arc_figures.py` draws figures on paper
`#f6efe2` with ink `#2b241c` and one amber accent, and the reel export
(`ui/src/lib/export-kindle.ts`) sets prose in `Bookerly, Charter, Georgia` at a
38em measure. The reading system is a promotion, not a pivot.

## The abstraction: eight rules

Kindle's clarity is a discipline, not a trait. Each rule is a constraint to
check a screen against.

**1. Two fonts, strictly divided.**
Anything a person reads is set in a book serif; anything a person operates is
set in a humanist sans. The two never mix inside one block of text.
*Why:* the font tells you, before you read a word, whether this is the story or
the controls.
*In OpenStory:* Story cards, reel captions, narration, transcript prose, and
session titles are serif. Tabs, buttons, chips, footers, and tables are sans.
Code stays mono.

**2. Paper and ink, in named themes.**
Surfaces are paper, text is ink, and each pairing has a name a person can pick:
Sepia, White, Black, Green. No theme is "light" or "dark"; it is a paper.
*Why:* naming the paper makes the theme a reading choice rather than a system
setting, and it forbids the unnamed near-black and pure-white defaults.
*In OpenStory:* the sepia light experiment becomes the default paper. The
Tokyonight dark theme becomes Black paper with cream ink. `data-theme` takes
four values.

**3. Secondary text lives in the gray band.**
Anything that is not the primary fact is set in one mid ink, never in full ink
and never in a third gray.
*Why:* Kindle's KDP rules permit one band of gray (`#666` to `#999`). One band
makes hierarchy binary: this is the thing, that is about the thing.
*In OpenStory:* `--ink-2` is the only muted color. Timestamps, branch names,
counts, and bylines all use it. Weight, not a third gray, carries any further
distinction.

**4. Stepped controls, saved as presets.**
Reading settings move in named steps, not sliders, and a set of steps can be
saved under a name.
*Why:* steps are reproducible and can be spoken aloud ("Large"); sliders
produce a thousand unshareable states.
*In OpenStory:* S/M/L/XL stays and becomes a root font-size change instead of
CSS `zoom`. Line spacing and margin get three steps each. The tuple (paper,
size, spacing, margin) is a saved reading theme.

**5. Chrome hides.**
On a reading surface, tabs, rails, and toolbars fold away once reading begins;
a tap or mouse-move at the top brings them back. What remains is one thin
footer: where you are on the left, percent on the right.
*Why:* Bezos' brief was a device that disappears in your hands. Chrome that is
always present is chrome you are always ignoring.
*In OpenStory:* the reel player is first (decision 6 of the export spec). The
Story reading view follows. "Stop 4 of 12" left, "33%" right.

**6. Marginalia, not overlays.**
Annotation sits in the text the way a highlighter sits on a page: a tinted band
behind a run of words, a dotted underline with a count, a note in the margin.
Never a floating panel over the page.
*Why:* a highlight is part of the reading; an overlay is an interruption of it.
*In OpenStory:* glass ink and beat ink map to four highlight tints. Sentence
patterns that recur across sessions get the dotted "popular" underline with a
count.

**7. Color the Colorsoft way.**
Color is desaturated and print-like. One accent does the work. Status colors
(error, success) are reserved and rare. Charts use the accent plus ink.
*Why:* Colorsoft reviewers called the result "subtle, pale even, like
newspaper CMYK", and that is the compliment. Saturated hue on paper reads as
a sticker.
*In OpenStory:* the ten session identity colors are desaturated by default with
a Vivid switch, exactly as Kindle offers Standard and Vivid.

**8. The e-ink constraint is the aesthetic.**
Relative units only. No motion beyond a page turn. No gradients, glows, blur,
or drop shadows. If it would not survive Send to Kindle, it is not allowed on a
reading surface.
*Why:* the constraint produces the calm. Every effect you cannot use is one the
reader never has to look past.
*In OpenStory:* `rem` and `em` on reading surfaces; the only transition is a
120ms opacity on chrome appearing; `backdrop-blur` and `shadow-2xl` leave the
reel player.

## Tokens

### Papers and inks

Sepia is the default. Highlights are per paper because a marker on black paper
is a darker tint. Contrast on every paper: ink above 12:1, secondary ink above
4.5:1, accent above 3:1 (the threshold `arc_figures.py` validated).

| Token | Sepia (default) | White | Black | Green | Existing var | Replaces |
|---|---|---|---|---|---|---|
| `--paper` | `#f6efe2` | `#fcfbf8` | `#141414` | `#e8efdf` | `--bg` | `--bg`, `--bg-surface` on reading surfaces |
| `--paper-2` | `#efe6d4` | `#f3f1ec` | `#1e1e1d` | `#dde6d1` | `--bg-surface` | `--bg-surface`, `--bg-hover` |
| `--ink` | `#2b241c` | `#1f1d1a` | `#d8d3c8` | `#26291f` | `--text` | `--text`, `--text-bright` |
| `--ink-2` | `#6b5d4d` | `#6e6a63` | `#8f8a80` | `#66705a` | `--text-muted` | `--text-muted` |
| `--rule` | `#d9cfbd` | `#e2ded6` | `#2b2a27` | `#cbd5bd` | `--divider` | `--divider`, `--border` |
| `--accent` | `#c25a1e` | `#b8501a` | `#e08a5a` | `#b4501c` | `--accent` | `--accent` (was blue `#7aa2f7` / `#1a5ab8`) |
| `--hl-yellow` | `#ecd98c` | `#f0e29a` | `#4d4420` | `#e3d88a` | none | new |
| `--hl-pink` | `#e8b9c1` | `#eec3ca` | `#4a2f35` | `#e0b7bd` | none | new |
| `--hl-blue` | `#b9cde0` | `#c2d5e6` | `#2b3a4a` | `#b4c9d8` | none | new |
| `--hl-orange` | `#eec39c` | `#f2cba6` | `#4e3520` | `#e8bf98` | none | new |

`--stream` (the dark field behind Live events) survives, because Live is an
instrument. The ten `--sc-*` session colors survive, desaturated by default
(rule 7). `--green`, `--red`, `--orange`, `--purple`, `--cyan` become status
colors only.

### Type

Bookerly and Amazon Ember are not licensable. Charter ships with macOS and iOS
and is the closest stand-in; Iowan Old Style is second; Georgia is everywhere.
If a web font is ever bundled, it is Literata.

```
--font-read: Charter, "Iowan Old Style", Literata, Georgia, serif;
--font-ui:   system-ui, -apple-system, "Segoe UI", Roboto, "Helvetica Neue", sans-serif;
--font-code: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
```

Kindle's fourteen sizes are for one paragraph at a time. A UI needs five steps;
S/M/L/XL changes the root, not the steps.

| Token | Size | Line-height | Role | Replaces |
|---|---|---|---|---|
| `--t-caption` | `0.75rem` | 1.4 | footers, timestamps, chips | `--fs-label` (10px, too small) |
| `--t-ui` | `0.8125rem` | 1.45 | instrument body text | `--fs-body` (12px) and `--fs-emph` (13px) |
| `--t-read` | `1.0625rem` | 1.6 | reading body | new |
| `--t-title` | `1.375rem` | 1.25 | card and section titles | `--fs-headline` (18px) |
| `--t-display` | `2rem` | 1.15 | reel title beats, story headings | new |

Root font-size by control step: S `15px`, M `16px`, L `18px`, XL `20px`. The
existing `zoom` values (0.9, 1.0, 1.15, 1.3) are retired once components are on
`rem`.

Measure: reading surfaces are capped at `38em`, about 66 characters at the
reading size. `export-kindle.ts` already uses this number. Justified text with
hyphenation is allowed on reading surfaces (Kindle's default) and forbidden on
instrument surfaces.

### Spacing and radius

Spacing steps: `0.25rem`, `0.5rem`, `1rem`, `1.5rem`, `2rem`, `3rem`.

Radius: a page has no corners. Reading surfaces use `0`. Highlight bands use
`2px` so the tint reads as marker, not box. Instrument controls keep `6px`.
`rounded-xl` cards and `rounded-full` pills leave the reading surfaces.

## Surface classes

Nielsen's finding: Kindle's page is worth copying, its navigation is not.
Linear reading is excellent; moving around non-linear content is awkward.
OpenStory has both kinds of screen and treats them differently on purpose.

**Reading surfaces get the full treatment** (all eight rules): the Story tab
(`components/story/`), the reel player (`components/reels/`), the Kindle
edition (`lib/export-kindle.ts`), transcript prose (`components/conversation/`
and Explore's conversation view), and session cards and rows
(`components/session/SessionCard.tsx`, the rows in `Sidebar.tsx`). A person
reads these top to bottom asking "what happened."

**Instrument surfaces get paper, ink, and fonts only** (rules 1, 2, 3, 7):
Explore (`components/explore/`), Canvas (`components/canvas/`), the Live
timeline and stream field (`Timeline.tsx`, `components/events/`), analytics,
viz, and Admin. They keep their interaction model: dense rows, hover, pan and
zoom, live updates, charts. They do not hide chrome, justify text, or cap the measure. Putting a
book page's manners on a faceted search would reproduce the Kindle weakness the
research warned about.

The test for a new screen: if a person could read it on a Kindle, it is a
reading surface.

## The mark

The brief: a book logo, clear and simple, like Apple. One closed shape, one
color, no gradient, working at 16px and 512px, reading as "book" and "open" at
once, with the mirror idea present but not clever. Three directions:

**Two leaves.** The classic open-book glyph, two pages with ruled lines and a
spine. Rejected: every reading app has it, and the lines die at 16px.

**Closed volume.** A closed book from the front, spine on the left. Quiet and
Apple-like. Rejected: it says "closed" and reads as a door at small sizes.
OpenStory is open by name and by license.

**The gutter.** One closed silhouette of an open book, no interior strokes: the
top edge dips to the gutter, the bottom edge dips to the spine, the outer edges
curl. The spine is the mirror axis; the right page is the left page reflected,
the mirror idea stated as geometry rather than drawn. The gutter is a hairline
slit in the outline that vanishes at 16px and appears at 64px and above.
Chosen. One path, `currentColor`, carrying the product's one idea without an
illustration.

The file is `docs/design/openstory-mark.svg`, viewBox `0 0 64 64`. In the
header it is set in ink at 20px next to the wordmark in the reading serif.

## Migration

Ten PR-sized steps, in order. "Tokens" means a CSS change in `index.css` with
no component edits; "components" means TSX changes.

1. **Reel player adopts the aesthetic** (components: `ReelsView.tsx`,
   `ReelBeatStage.tsx`). Decided in the export spec. Paper and ink from the
   theme, serif captions, chrome hidden during playback, the thin footer
   ("stop 4 of 12" left, "33%" right), figure beats framed image-title-caption.
   Remove `backdrop-blur`, `shadow-2xl`, `rounded-xl`.
2. **Promote sepia to default and add the papers** (tokens, plus
   `ThemeToggle.tsx`). Introduce `--paper`, `--ink`, `--ink-2`, `--rule`, the
   four highlight tints, and alias the old names to them. `data-theme` takes
   `sepia | white | black | green`; the toggle becomes a paper picker.
3. **Type tokens and the rem scale** (tokens, plus `TextSizeControl.tsx`).
   Add `--font-read`, `--font-ui`, `--font-code` and the five `--t-*` steps.
   The control sets root font-size instead of `zoom`.
4. **Story tab as a reading surface** (components: `StoryView.tsx`,
   `TurnCard.tsx`, `CycleCard.tsx`). Serif prose at the 38em measure, radius 0,
   secondary text in `--ink-2` only.
5. **Session rows as books on a shelf** (components: `SessionCard.tsx`,
   `Sidebar.tsx`). One primary fact in the serif, everything else in the gray
   band in the sans; `px` to `rem`. This also delivers DESIGN.md's rule 2.
6. **Highlights** (components: `BeatInkLayer.tsx`, `streams/draw`). Glass ink
   and beat ink pick from the four tints; the dotted popular underline with a
   count on recurring sentences.
7. **Instrument surfaces take paper and ink** (mostly tokens). Explore, Canvas,
   Live, Admin already consume `var(--bg)` and friends, so the alias in step 2
   does most of it. Charts move to accent plus ink; a two-line change per
   Recharts series.
8. **Standard and Vivid** (tokens, plus one toggle). Desaturated `--sc-*`
   session colors by default, the current values under a Vivid switch.
9. **Chrome hides on Story** (components: `StoryView.tsx`, `Header.tsx`,
   `TabBar.tsx`). Lift the reel player's hidden-chrome mode into a shared
   reading-mode hook.
10. **The mark in the header** (components: `Header.tsx`, `index.html`
    favicon). Inline the SVG at 20px beside the wordmark, set the favicon.

Steps 2, 3, 7, and 8 are token-led; the rest touch components. Each step has a
before-and-after screenshot, which is the review.

## Non-goals

Do not copy the Kindle store, its home screen, its menus, or the 2025 firmware
that shrank the page to fit chrome above and below. Do not copy X-Ray or Page
Flip as features; their ideas may return in OpenStory's own form. Do not add
fourteen text sizes. Green paper is in the table so the system is complete, not
because it earns a toggle today.

Do not remove what OpenStory has that Kindle lacks. Live data keeps flowing.
Graphs and canvases keep pan and zoom. The dark stream field behind Live events
stays dark in every paper, because instruments read against a field, not a
page. Search stays faceted. The mirror shows history as it is written; Kindle
never had to.
