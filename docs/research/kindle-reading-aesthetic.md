# The Kindle reading aesthetic, with color: a research note

**Status:** research, 2026-09-23. Not a decision. Written to answer one question
Max asked: *if we adopted the Kindle design aesthetic, but with color enabled,
what would we actually be adopting, and does it fit OpenStory?*

Sources were gathered by a read-only research pass. Claims marked **[V]** were
verified against the cited page; **[R]** are commonly reported and were not
confirmed against Amazon's own help pages (which returned 503 during the pass).
Amazon publishes no design-language spec for Kindle; what exists is the Kindle
Publishing Guidelines, the Kindle Create themes, and the device itself.

## Why Kindle fits OpenStory

OpenStory's soul doc calls the product a mirror: calm, showing the reflection and
not the wiring. `docs/DESIGN.md` says the failure mode we are correcting is *the
machine's numbers, not the human's story* on stage. Kindle's founding principle is
the same sentence from the other side. Bezos, at the 2007 launch: the device should
"disappear in your hands, to get out of the way, so you can enjoy your reading";
in 2014, "make the device disappear, so you can lose yourself in the author's
world." **[V]** https://www.aboutamazon.com/news/devices/a-look-back-at-10-years-of-the-amazon-kindle

The Reels page already treats a session as something to be *read*: opener, ordered
stops with narration, closer, a reading edition for Kindle (the Codex branch
`codex/kindle-reel-reports`). Adopting the reading aesthetic across the UI is not a
restyle; it is taking the Story and Reels tabs' premise seriously everywhere.

## What "Kindle" is made of

### Typography

- **Bookerly**, the reading serif. Designed by Dalton Maag for Amazon, 2015,
  replacing Caecilia; shipped with a new typesetting engine (justification,
  kerning, hyphenation, drop caps). **[V]** https://en.wikipedia.org/wiki/Bookerly
- **Amazon Ember**, the UI sans. Dalton Maag, 2015, debuted on Oasis 2016; used
  for menus and chrome, never body text. **[V]** https://fontsinuse.com/typefaces/167735/amazon-ember
- Bundled reading fonts on the device: Amazon Ember, Baskerville, Bookerly,
  Caecilia, Caecilia Condensed, Futura, Helvetica, OpenDyslexic, Palatino.
  **[V]** https://www.howtogeek.com/734656/how-to-customize-text-on-your-kindle/
- Controls are **stepped, not continuous**: 14 font sizes, 5 boldness levels, 3
  line spacings, 3 margins, justified or ragged, saved as named presets
  (Compact, Standard, Large, Low Vision, plus user themes). **[V]** same source, and
  https://www.idownloadblog.com/2020/12/17/create-use-themes-kindle-paperwhite/
- Justified is the platform default for reflowable books. **[V]**
  https://www.ebookpbook.com/2026/04/11/epub-to-kindle-conversion/
- Neither Bookerly nor Ember is licensed for third parties. Open stand-ins that
  read as the same family: **Literata** or **Source Serif 4** for body (Literata is
  Google Play Books' default, not a Kindle font **[R]**), and a humanist sans such as
  **Source Sans 3** or **Inter** for chrome. On macOS, **Charter** is a close local
  stand-in; `scripts/arc_figures.py` uses it for figure text.

### Color and themes

- App themes: **White, Sepia, Green, Black**. iOS dark mode auto-switches to Black;
  Sepia and Green do not participate in auto-switching. **[V]**
  https://geekupdated.com/how-to-control-theme-colors-kindle-for-ipad-iphone/
- No official hex values exist. Sampled values, all unofficial:
  sepia paper `#FBF0D9` with ink `#5F4B32` **[R]**
  https://medium.com/greatnote/kindle-sepia-color-code-1fed14b1a5ef ;
  paper `rgb(231,222,199)` with ink `rgb(93,66,50)` sampled from a Fire HDX
  screenshot **[V]** https://www.mobileread.com/forums/showthread.php?t=230668 ;
  a Cloud Reader bookmarklet uses paper `rgb(246,239,220)`, ink `rgb(64,41,25)`,
  highlight `rgb(255,245,173)`, night paper `rgb(57,59,61)` with ink
  `rgb(203,207,208)` **[V]** https://gist.github.com/YuriyGuts/2d84a34efac32069aab1
- The KDP rules are the closest thing to a published palette law: body text at
  1em with no imposed color; grays only in the `#666` to `#999` band; "body text
  must not have a black or white background color." **[V]**
  https://kdp.amazon.com/en_US/help/topic/GH4DRT75GWWAGBTU
- **Colorsoft** is what "Kindle with color" literally means: Kaleido 3 panel, 300
  ppi monochrome and 150 ppi color, "over 4,096 colors" **[R]**; Amazon's framing is
  "rich, paper-like color" and "vibrant yet easy on the eyes" **[V]**
  https://blog.the-ebook-reader.com/2024/10/16/new-kindle-colorsoft-details-summary-first-color-kindle/
  and https://www.aboutamazon.com/news/devices/kindle-colorsoft-color-comics-manga-novels
- Two color styles, **Standard** (full range, duller) and **Vivid** (more saturation
  and contrast, fewer colors). **[V]** https://www.pocket-lint.com/when-to-use-vivid-mode-on-a-kindle/
- Reviewers describe the result as "subtle, pale even," like newspaper CMYK.
  **[V]** https://techcrunch.com/2024/10/30/kindle-colorsoft-review-a-subtle-approach-to-color
- Highlights: yellow, pink, blue, orange, filterable by color; green highlights and
  colored bookmarks added in 2026. **[V]** aboutamazon link above; **[R]**
  https://blog.the-ebook-reader.com/2026/02/19/kindle-colorsoft-now-has-colored-bookmarks-and-green-highlights/

### Layout and chrome

- Chrome hides while reading; a tap at the top reveals the toolbar. **[R]**
- The footer is a thin line: progress on the left, cycling on tap through page,
  time left in chapter, time left in book, location, or off; percent on the right.
  **[R]** https://www.makeuseof.com/show-reading-progress-kindle/
- The **Aa** menu opens in the lower half so changes preview live in the upper half;
  tabs are Themes, Font, Layout, More. **[V]**
  https://blog.the-ebook-reader.com/2020/04/09/this-is-what-the-kindles-new-aa-menu-looks-like/
- **Popular Highlights** are dotted underlines with a count ("386 Highlighters").
  **[V]** https://www.howtogeek.com/355701/how-to-turn-off-popular-highlights-on-your-kindle/
- **Page Flip** pins the current page and lets you browse pixel-accurate
  thumbnails in a grid; the design cue was "observing how people read print
  editions." **[V]** https://techcrunch.com/2016/06/28/amazon-introduces-page-flip-for-kindle/
- **X-Ray** tabs: People, Terms, Notable Clips, Images. **[R]**

### What survives Send to Kindle

This matters because the Kindle reading edition is our export target.

- KF8 CSS support: color, font family/size/weight/style, text-align, line-height,
  text-indent, margin, padding, border, background color and image, float,
  position, `@font-face`. Not supported: max-width and max-height, `::before` and
  `::after`, `:nth-child`, and no audio, video, canvas, or iframe. **[V]**
  https://kdp.amazon.com/en_US/help/topic/GG5R7N649LECKP7U
- Conversion: transparency becomes white, CMYK becomes sRGB, columns and grids are
  effectively dropped, embedded fonts need reader opt-in. **[V]**
  https://www.ebookpbook.com/2026/04/11/epub-to-kindle-conversion/
- Send to Kindle by email takes 50 MB; EPUB is converted server-side; complex CSS
  such as tables and columns "often doesn't" survive. **[V]** https://epublys.com/how-to-send-epub-to-kindle

### Critiques worth keeping in view

1. Nielsen, 2009: page turning is the best interface feature; moving around
   non-linear content is awkward. **[V]** https://www.nngroup.com/articles/kindle-content-design/
2. Nielsen, Kindle 2: reading speed within 0.5 percent of print, but "awkward
   pointing plus slow reaction equals a bad user experience." **[V]**
   https://www.nngroup.com/articles/kindle-2-usability-review/
3. A 2019 heuristic evaluation gave industrial design five stars and visual,
   information architecture, and interaction three: no tap feedback, tiny nav bars.
   **[V]** https://www.linkedin.com/pulse/case-study-ux-design-evaluation-kindle-paperwhite-bear-liu
4. The 2025 firmware shrank the page to fit menus above and below instead of
   overlaying, and readers found it slower and less informative. **[V]**
   https://blog.the-ebook-reader.com/2025/09/05/the-constant-random-kindle-ui-changes-are-really-obnoxious/

The lesson from the critiques: the reading page is the part to copy. Kindle's
navigation, store, and menus are not admired by anyone. OpenStory's Explore and
Canvas tabs are non-linear by nature, exactly where Kindle is weakest.

## What a designer would copy: eight decisions

1. **Two fonts, strictly divided.** A book serif for anything read (Story cards,
   reel captions, transcript prose, narration). A humanist sans for chrome only.
   Never mixed inside body text.
2. **Warm paper, warm ink.** A sepia default around `#F6EFE2` with ink around
   `#2B241C`, a white variant, a near-black variant, and a green variant. The
   existing light theme already went sepia in July (`b7fb5ca feat(ui): sepia
   light-mode experiment`), so this is a promotion, not a pivot.
3. **Secondary text lives in the gray band.** Muted labels in the `#666` to `#999`
   range on paper, never pure black on white.
4. **Stepped controls saved as presets.** The header text-size control (S, M, L,
   XL) is already stepped; add line-spacing and margin steps and let the four
   values be saved as a named reading theme.
5. **Chrome that hides.** In a reel or Story reading view, the tab bar and rails
   fold away; a tap or mouse-move at the top brings them back. The footer becomes
   a single thin line: where you are on the left, percent on the right.
6. **Annotation as marginalia.** Glass ink and beat annotations map to the four
   muted highlight tints; sentence patterns that recur across sessions could get
   the dotted "popular" underline with a count.
7. **Color the Colorsoft way.** Desaturated, print-like, one accent doing the
   work. Charts use one hue plus ink; status colors stay reserved. Offer a
   Standard and a Vivid switch rather than a saturated default.
8. **The e-ink constraint is the aesthetic.** Relative units, no motion beyond a
   page turn, no gradients, no glows. What survives Send to Kindle is what the UI
   is allowed to use.

## What it would touch in OpenStory

- `ui/src/index.css` color tokens: the sepia light theme becomes the default
  paper; add `--paper-white`, `--paper-green`, `--paper-black` variants under the
  existing theme toggle.
- `ui/src/components/reels/` and the Story tab: serif body, hidden chrome mode,
  thin footer, marginalia highlights.
- `scripts/arc_figures.py` is the first artifact drawn to these rules and can be
  the palette's reference instance for charts.
- The Kindle reading edition (`rs/store/src/reel_report.rs` on the Codex branch)
  is the constraint check: if a screen cannot be expressed in KF8 CSS, it is
  probably too clever for the reading view.

## Open questions

- Which open serif: Literata (widest weights, Google-hosted) or Source Serif 4
  (closer to Bookerly's color)? Decide by rendering one Story card in each.
- Does a green paper variant earn its place, or is it Kindle trivia?
- Explore and Canvas are not reading surfaces. Do they get paper and ink only, or
  the full treatment? Recommendation: paper, ink, and fonts only; keep their
  interaction model.
