"""Build a personalised "OpenStory, exercised" report from YOUR real data.

Sibling of build_prompt_library.py. Where the prompt library shows *illustrative*
examples, this renders the *real* output of running each of the twelve
openstory-skills against your live store — one section per skill, charted with
the same vocabulary (stats / bars / table / timeline / …) and theme.

Data in, report out. The numbers live in a JSON file (gathered by querying the
OpenStory MCP/REST API — see scripts/, or the fan-out that produced
drafts/skill_report_data.json); this script only renders. That keeps it a pure,
re-runnable transform: regenerate the page/PDF any time the data file changes.

    --data PATH   the report data JSON (default: drafts/skill_report_data.json)
    --html PATH   the report web page
    --pdf  PATH   print-ready PDF (via headless Chrome)
    --theme NAME  palette (default: georgetown)
    --test        self-checks against a tiny fixture

Data JSON shape:
    {
      "title": "...", "subtitle": "...", "window": "last 30 days",
      "sections": [
        {"ix":"01","color":"blue","title":"/cost · what your sessions cost",
         "prompt":"What have my agent sessions cost me?",
         "source":"token_usage + daily_token_usage",
         "viz":[ <block>, ... ]}
      ]
    }
Each <block> is exactly a build_prompt_library viz block (stats/bars/table/…).
"""

import argparse
import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

# Reuse the chart engine + theme + PDF renderer from the sibling generator.
sys.path.insert(0, str(Path(__file__).resolve().parent))
from build_prompt_library import _block, _e, root_css, _CSS, THEMES, chrome_pdf, leakage  # noqa: E402

_COLORS = {"blue", "cyan", "purple", "green", "orange", "red"}

# Report-specific styling layered on top of the shared chart CSS: the first-page
# skills index, and a print rule that starts every skill on its own page.
_REPORT_CSS = """
  .index { margin:36px 0 0; }
  .index > h2 { font-family:var(--fdisplay); font-size:20px; font-weight:700; letter-spacing:-.01em; margin:0 0 4px; }
  .index .lede { color:var(--dim); font-size:13.5px; margin:0 0 16px; }
  .idx-grid { display:grid; grid-template-columns:1fr 1fr; gap:0 32px; }
  .idx-item { display:flex; align-items:baseline; gap:11px; padding:7px 0; border-bottom:1px solid var(--line); }
  .idx-item .idx-n { flex:none; width:22px; font:700 12px/1.4 "JetBrains Mono",monospace; color:var(--ac); font-variant-numeric:tabular-nums; }
  .idx-item .idx-cmd { flex:none; font:700 13px/1.4 "JetBrains Mono",monospace; color:var(--ink); }
  .idx-item .idx-d { color:var(--dim); font-size:13px; line-height:1.4; }
"""

_SHELL = """<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="{fontlink}" rel="stylesheet">
<style>{css}</style></head>
<body><div class="wrap">
{inner}
</div></body></html>"""


def _shell(inner: str, data: dict, theme: str) -> str:
    return _SHELL.format(css=root_css(theme) + _CSS + _REPORT_CSS,
                         fontlink=THEMES[theme]["link"],
                         title=_e(data.get("title", "Your OpenStory")), inner=inner)


def _header_html(data: dict) -> str:
    title = data.get("title", "Your OpenStory")
    grad = data.get("title_grad", "")
    title_html = _e(title)
    if grad and grad in title:
        title_html = _e(title).replace(_e(grad), f'<span class="grad">{_e(grad)}</span>', 1)
    window = data.get("window", "")
    window_html = f' over <strong>{_e(window)}</strong>' if window else ""
    return (
        '<header><p class="eyebrow">OpenStory · your data</p>'
        f'<h1>{title_html}</h1><p class="sub">{_e(data.get("subtitle", ""))}</p>'
        '<div class="howto"><strong>How this was made:</strong> each section is the real '
        'output of one of the twelve <code>openstory-skills</code> run against your live '
        f'store{window_html} — the same reports, charted from your own history. Names of '
        'collaborators are shown as roles; secrets are never included.</div></header>'
    )


def _index_html(data: dict) -> str:
    rows = []
    for s in data["sections"]:
        cmd, _, desc = s["title"].partition(" · ")
        rows.append(
            f'<div class="idx-item {s["color"]}"><span class="idx-n">{_e(s["ix"])}</span>'
            f'<span class="idx-cmd">{_e(cmd)}</span><span class="idx-d">{_e(desc)}</span></div>'
        )
    return (
        '<nav class="index"><h2>The twelve skills</h2>'
        '<p class="lede">Each one is a Claude Code slash command (the <code>openstory-skills</code> '
        'plugin). One per page that follows — with your real numbers.</p>'
        f'<div class="idx-grid">{"".join(rows)}</div></nav>'
    )


def _section_html(s: dict) -> str:
    if s.get("color") not in _COLORS:
        raise ValueError(f"section {s.get('ix')!r}: color must be one of {_COLORS}")
    viz = "".join(_block(b) for b in s["viz"])
    return (
        f'<section class="{s["color"]}"><div class="sechead">'
        f'<span class="ix">{_e(s["ix"])}</span><h2>{_e(s["title"])}</h2></div>'
        f'<p class="why">{_e(s.get("prompt", ""))}</p>'
        f'<div class="viz">{viz}<div class="src">{_e(s.get("source", ""))}</div></div></section>'
    )


_FOOT = ('<div class="draftnote">Generated by <span class="mono">scripts/build_skill_report.py</span> '
         'from <span class="mono">drafts/skill_report_data.json</span> — re-run any time to refresh.</div>'
         '<footer>OpenStory · a mirror, not a leash.</footer>')


def render_html(data: dict, theme: str = "georgetown") -> str:
    """The web page: header + index + every section in one scrolling document."""
    if theme not in THEMES:
        raise ValueError(f"unknown theme {theme!r}; choose from {list(THEMES)}")
    body = _header_html(data) + _index_html(data) + "".join(_section_html(s) for s in data["sections"]) + _FOOT
    return _shell(body, data, theme)


def pdf_pages(data: dict, theme: str = "georgetown") -> list[str]:
    """One self-contained HTML document per PDF page: page 1 is the header + skills
    index, then one page per skill section. Each is short enough that headless
    Chrome paginates it cleanly — unlike one tall doc with many forced breaks,
    which Chrome truncates. Merged into the final PDF with pdfunite."""
    if theme not in THEMES:
        raise ValueError(f"unknown theme {theme!r}; choose from {list(THEMES)}")
    pages = [_shell(_header_html(data) + _index_html(data), data, theme)]
    pages += [_shell(_section_html(s) + _FOOT if s is data["sections"][-1] else _section_html(s),
                     data, theme) for s in data["sections"]]
    return pages


def render_pdf(data: dict, out_path: str, theme: str = "georgetown") -> None:
    """Render one page per skill (plus the index page) and merge with pdfunite, so
    each question lands on its own page. Falls back to a single document if
    pdfunite isn't installed (then the report is one continuous flow)."""
    out = Path(out_path)
    out.parent.mkdir(parents=True, exist_ok=True)
    merge = shutil.which("pdfunite")
    if not merge:
        chrome_pdf(render_html(data, theme), out_path)
        print(f"Wrote {out_path} [{theme}] — single flow (install pdfunite for one page per skill)")
        return
    pages = pdf_pages(data, theme)
    with tempfile.TemporaryDirectory() as tmp:
        parts = []
        for i, page_html in enumerate(pages):
            part = Path(tmp) / f"p{i:02d}.pdf"
            chrome_pdf(page_html, str(part))
            parts.append(str(part))
        subprocess.run([merge, *parts, str(out)], check=True, capture_output=True)
    print(f"Wrote {out_path} [{theme}] — {len(pages)} pages (1 index + {len(pages) - 1} skills)")


def run_tests() -> None:
    print("Running tests...")
    fixture = {
        "title": "Your work, measured", "title_grad": "measured",
        "subtitle": "A report from your own history.", "window": "last 30 days",
        "sections": [
            {"ix": "01", "color": "blue", "title": "/cost · what your sessions cost",
             "prompt": "What have my agent sessions cost me?", "source": "token_usage",
             "viz": [{"type": "stats", "items": [{"num": "12.4M", "label": "tokens", "accent": True},
                                                 {"num": "$58", "label": "est. cost"}]},
                     {"type": "note", "text": "**Cache** saved most of the spend."}]},
            {"ix": "02", "color": "green", "title": "/coach · how you work",
             "prompt": "Honest feedback?", "source": "session_patterns",
             "viz": [{"type": "bars", "title": "themes", "items": [{"label": "docs", "bar": 60, "val": "60%"}]}]},
        ],
    }
    h = render_html(fixture)
    assert h.startswith("<!doctype html>") and h.rstrip().endswith("</html>")
    assert h.count("<section") == 2, "one section per skill"
    assert "12.4M" in h and "$58" in h, "real numbers rendered"
    assert '<span class="grad">measured</span>' in h, "title gradient span"
    # first-page index: one row per skill, with command + quick description
    assert h.count('class="index"') == 1 and h.count('class="idx-item') == 2, "index lists every skill"
    assert '<span class="idx-cmd">/cost</span>' in h, "index shows the command"
    assert "what your sessions cost" in h, "index shows the quick description"
    assert h.index('class="index"') < h.index("<section"), "index precedes the sections"
    for klass in ("vstats", "vbars", "vnote"):
        assert klass in h, f"missing chart primitive {klass}"
    assert ":root{" in h and "--bg:" in h, "theme tokens present"

    # paged PDF: one self-contained document per page (index page + one per skill)
    pages = pdf_pages(fixture)
    assert len(pages) == 3, f"index + 2 skills = 3 pages, got {len(pages)}"
    assert all(p.startswith("<!doctype html>") and p.rstrip().endswith("</html>") for p in pages)
    assert 'class="index"' in pages[0] and "<section" not in pages[0], "page 1 is the index, no section"
    assert pages[1].count("<section") == 1 and "12.4M" in pages[1], "page 2 is the first skill"
    assert "draftnote" in pages[-1], "footer rides the last page"

    # color validation
    try:
        render_html({"title": "t", "sections": [{"ix": "x", "color": "teal", "title": "t", "viz": []}]})
        raise AssertionError("should reject unknown color")
    except ValueError:
        pass
    assert not leakage(h), f"leakage: {leakage(h)}"
    print(f"  OK: report renders {h.count('<section')} sections + {len(pages)}-page paged PDF, no leakage")
    print("\nAll tests passed.")


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--data", metavar="PATH", default="drafts/skill_report_data.json")
    ap.add_argument("--html", metavar="PATH")
    ap.add_argument("--pdf", metavar="PATH")
    ap.add_argument("--theme", default="georgetown", choices=list(THEMES))
    ap.add_argument("--test", action="store_true")
    args = ap.parse_args()
    if args.test:
        run_tests(); sys.exit(0)
    if not (args.html or args.pdf):
        ap.error("pass --html PATH and/or --pdf PATH (or --test)")
    data = json.loads(Path(args.data).read_text(encoding="utf-8"))
    leaks = leakage(json.dumps(data))
    if leaks:
        ap.error(f"refusing to render — possible secrets in data: {leaks}")
    if args.html:
        Path(args.html).write_text(render_html(data, args.theme), encoding="utf-8")
        print(f"Wrote {args.html} [{args.theme}]")
    if args.pdf:
        render_pdf(data, args.pdf, args.theme)
