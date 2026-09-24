#!/usr/bin/env python3
"""Turn a saved reel into a slide deck: one 16:9 page per beat, printed to PDF.

Why: a reading edition is right for a Kindle, but a friend on iMessage wants
something that previews inline and swipes. A PDF deck does that with nothing
installed. The deck keeps the reel's paper-and-ink aesthetic (warm paper, book
serif, one accent) and drops the machine words: no "Part 10", no UUIDs on the
page. Every spotlight beat gets a title (the reel's `visual.title`, or one cut
from the narration), the narration as the main text, and, when the record is
reachable, a short quote of what was actually said, with its date.

Inputs
  --reel-id ID         fetch the reel from the API (default http://localhost:3002)
  --file PATH          or read the reel JSON from disk (data/reels/<id>.json)
  --quotes PATH        optional JSON {stopIndex: {"text": ..., "when": "YYYY-MM-DD"}}
                       when the record is not reachable over the API
  --out DIR            output directory (default exports/)
  --no-pdf             write the HTML only (skip Playwright)

Outputs
  <slug>.deck.html and <slug>.deck.pdf (via scripts/html_to_pdf.mjs, which
  uses the Playwright Chromium already installed for the E2E suite).

    python3 scripts/reel_slides.py --reel-id reel-... --out exports
    python3 scripts/reel_slides.py --test
"""
from __future__ import annotations

import argparse
import html
import json
import re
import subprocess
import sys
import urllib.request
from dataclasses import dataclass, field
from pathlib import Path

PAPER = "#f6efe2"
INK = "#2b241c"
INK_2 = "#6b5d4d"
RULE = "#d9cfbd"
ACCENT = "#c25a1e"


# ---------- pure model ----------

@dataclass(frozen=True)
class Slide:
    kind: str                 # cover | figure | text | closer
    title: str
    body: str = ""            # narration
    image: str = ""           # data URL for figure slides
    quote: str = ""           # what was said, for text slides
    when: str = ""            # date of the quoted event
    number: int = 0
    total: int = 0


def slug(title: str) -> str:
    s = re.sub(r"[^a-z0-9]+", "-", title.lower()).strip("-")
    return s or "reel"


def title_from_line(line: str, limit: int = 48) -> str:
    """A slide title cut from the narration when the author gave none: the
    first sentence, minus a leading story-spine stem, trimmed to `limit`."""
    first = re.split(r"(?<=[.!?])\s", line.strip(), maxsplit=1)[0]
    first = re.sub(r"^(Every day|Until one day|Because of that|Until finally|And|But|So|Therefore),?\s+", "", first, flags=re.I)
    first = first.rstrip(".!?")
    if len(first) <= limit:
        return first[:1].upper() + first[1:]
    cut = first[:limit].rsplit(" ", 1)[0]
    return (cut[:1].upper() + cut[1:]).rstrip(",;:") + "…"


def slides_from_reel(reel: dict, quotes: dict[int, dict] | None = None) -> list[Slide]:
    quotes = quotes or {}
    body: list[Slide] = []
    for i, stop in enumerate(reel.get("stops", [])):
        visual = stop.get("visual") or {}
        kind = stop.get("kind") or "spotlight"
        title = (visual.get("title") or "").strip() or title_from_line(stop.get("line", ""))
        if kind == "image" and visual.get("imageHref", "").startswith("data:image/"):
            body.append(Slide("figure", title, stop.get("line", ""), image=visual["imageHref"]))
        elif kind == "title":
            body.append(Slide("text", title, stop.get("line", "")))
        else:
            q = quotes.get(i) or {}
            body.append(Slide("text", title, stop.get("line", ""), quote=q.get("text", ""), when=q.get("when", "")))
    out: list[Slide] = [Slide("cover", reel.get("title", "Reel"), reel.get("opener", ""))]
    out += body
    if reel.get("closer"):
        out.append(Slide("closer", "", reel["closer"]))
    total = len(out)
    return [Slide(s.kind, s.title, s.body, s.image, s.quote, s.when, n + 1, total) for n, s in enumerate(out)]


# ---------- rendering ----------

def esc(s: str) -> str:
    return html.escape(s, quote=True)


def render_slide(s: Slide) -> str:
    num = f'<div class="num">{s.number} / {s.total}</div>' if s.kind not in ("cover",) else ""
    if s.kind == "cover":
        return f'''<section class="slide cover">
  <div class="rule"></div>
  <h1>{esc(s.title)}</h1>
  <p class="opener">{esc(s.body)}</p>
  <div class="foot">made with OpenStory · read your agent history</div>
</section>'''
    if s.kind == "figure":
        # The figure carries its own title inside the image; a heading above
        # it would say the same thing twice.
        return f'''<section class="slide figure">
  <div class="img"><img src="{s.image}" alt="{esc(s.title)}"></div>
  <p class="caption">{esc(s.body)}</p>
  {num}
</section>'''
    if s.kind == "closer":
        return f'''<section class="slide closer">
  <div class="rule"></div>
  <p class="closing">{esc(s.body)}</p>
  {num}
</section>'''
    quote = ""
    if s.quote:
        when = f'<span class="when">{esc(s.when)}</span>' if s.when else ""
        quote = f'<blockquote><p>{esc(s.quote)}</p><footer>from the record {when}</footer></blockquote>'
    return f'''<section class="slide text">
  <h2>{esc(s.title)}</h2>
  <p class="body">{esc(s.body)}</p>
  {quote}
  {num}
</section>'''


CSS = f"""
@page {{ size: 1600px 900px; margin: 0; }}
html, body {{ margin: 0; padding: 0; background: {PAPER}; color: {INK};
  font-family: Charter, "Iowan Old Style", Georgia, serif; }}
.slide {{ width: 1600px; height: 900px; box-sizing: border-box; padding: 88px 120px; position: relative;
  page-break-after: always; break-after: page; overflow: hidden; background: {PAPER}; }}
.slide:last-child {{ page-break-after: auto; break-after: auto; }}
h1 {{ font-size: 72px; line-height: 1.1; font-weight: normal; margin: 0 0 32px; max-width: 1200px; }}
h2 {{ font-size: 44px; line-height: 1.15; font-weight: normal; margin: 0 0 28px; color: {INK}; }}
.rule {{ width: 96px; height: 6px; background: {ACCENT}; margin-bottom: 40px; border-radius: 3px; }}
.opener {{ font-size: 30px; line-height: 1.45; color: {INK_2}; max-width: 1200px; margin: 0; }}
.body {{ font-size: 38px; line-height: 1.45; margin: 0 0 40px; max-width: 1340px; }}
blockquote {{ margin: 0; padding: 22px 0 0 36px; border-left: 6px solid {ACCENT}; max-width: 1240px; }}
blockquote p {{ font-size: 28px; line-height: 1.4; color: {INK_2}; margin: 0 0 12px; font-style: italic; }}
blockquote footer {{ font-size: 18px; color: {INK_2}; letter-spacing: .02em; }}
blockquote .when {{ color: {ACCENT}; margin-left: 8px; }}
.figure h2 {{ margin-bottom: 18px; }}
.figure .img {{ height: 660px; display: flex; align-items: center; justify-content: center; }}
.figure img {{ max-width: 100%; max-height: 100%; border-radius: 8px; }}
.caption {{ font-size: 22px; color: {INK_2}; margin: 18px 0 0; line-height: 1.35; max-width: 1360px; }}
.closer .closing {{ font-size: 56px; line-height: 1.2; margin: 0; max-width: 1200px; }}
.num, .foot {{ position: absolute; bottom: 44px; font-size: 18px; color: {INK_2}; }}
.num {{ right: 120px; }} .foot {{ left: 120px; }}
"""


def render_deck(slides: list[Slide], title: str) -> str:
    return f'''<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><title>{esc(title)}</title>
<style>{CSS}</style></head>
<body>
{chr(10).join(render_slide(s) for s in slides)}
</body></html>'''


# ---------- sources ----------

def fetch_json(url: str):
    with urllib.request.urlopen(url, timeout=30) as r:
        return json.load(r)


def load_reel(api: str, reel_id: str | None, file: str | None) -> dict:
    if file:
        return json.loads(Path(file).read_text())
    return fetch_json(f"{api}/api/reels/{reel_id}")


def print_pdf(html_path: Path, pdf_path: Path, repo_root: Path) -> None:
    subprocess.run(["node", str(repo_root / "scripts" / "html_to_pdf.mjs"), str(html_path), str(pdf_path)],
                   cwd=repo_root, check=True)


# ---------- main ----------

def run(a) -> Path:
    reel = load_reel(a.api, a.reel_id, a.file)
    quotes = None
    if a.quotes:
        raw = json.loads(Path(a.quotes).read_text())
        quotes = {int(k): v for k, v in raw.items()}
    slides = slides_from_reel(reel, quotes)
    out = Path(a.out); out.mkdir(parents=True, exist_ok=True)
    base = str(out / f"{slug(reel.get('title', 'reel'))}.deck")
    html_path = Path(base + ".html")
    html_path.write_text(render_deck(slides, reel.get("title", "Reel")), encoding="utf-8")
    print(f"wrote {html_path} ({len(slides)} slides)")
    if not a.no_pdf:
        print_pdf(html_path, Path(base + ".pdf"), Path(__file__).resolve().parent.parent)
    return html_path


def _test() -> int:
    assert title_from_line("Every day the fleet grew. In July it broke.") == "The fleet grew"
    assert title_from_line("Because of that, the company found its word. More.") == "The company found its word"
    t = title_from_line("Until finally the unfinished chapter surfaced with a very long tail that goes on and on")
    assert t.endswith("…") and len(t) <= 49, t
    reel = {"title": "T", "opener": "O", "closer": "C", "stops": [
        {"kind": "image", "line": "cap", "visual": {"title": "Fig", "imageHref": "data:image/png;base64,AAAA"}},
        {"sessionId": "s", "eventId": "e", "line": "Every day the fleet grew. Then more."},
        {"sessionId": "s", "eventId": "e2", "line": "x", "visual": {"title": "Given"}},
    ]}
    s = slides_from_reel(reel, {1: {"text": "it takes 4G of RAM?", "when": "2026-07-09"}})
    assert [x.kind for x in s] == ["cover", "figure", "text", "text", "closer"], [x.kind for x in s]
    assert s[1].title == "Fig" and s[1].image.startswith("data:image/png")
    assert s[2].title == "The fleet grew" and s[2].quote == "it takes 4G of RAM?" and s[2].when == "2026-07-09"
    assert s[3].title == "Given" and s[3].quote == ""
    assert (s[0].number, s[0].total, s[4].number) == (1, 5, 5)
    deck = render_deck(s, "T <deck>")
    assert "<title>T &lt;deck&gt;</title>" in deck and deck.count('<section class="slide') == 5
    assert "Part " not in deck and "<script" not in deck
    assert "4 / 5" in deck and "from the record" in deck
    assert slug("The Arc of OpenStory, illustrated") == "the-arc-of-openstory-illustrated"
    print("ok: 12 assertions")
    return 0


def main(argv=None) -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--reel-id")
    p.add_argument("--file")
    p.add_argument("--quotes")
    p.add_argument("--api", default="http://localhost:3002")
    p.add_argument("--out", default="exports")
    p.add_argument("--no-pdf", action="store_true")
    p.add_argument("--test", action="store_true")
    a = p.parse_args(argv)
    if a.test:
        return _test()
    if not (a.reel_id or a.file):
        p.error("--reel-id or --file is required")
    run(a)
    return 0


if __name__ == "__main__":
    sys.exit(main())
