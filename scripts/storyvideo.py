#!/usr/bin/env python3
"""Generate a HyperFrames daystory video composition from OpenStory data.

Two modes:

  Plan mode (recommended) — agent-authored narrative
    1. Collect facts: /api/sessions + sessionstory.py --json per session
    2. Read a scene plan (JSON, hand-authored or model-authored)
    3. Render scenes to a HyperFrames composition

  Raw mode — mechanical sentence sampling (testing only)
    Same facts collection, but body cards are sampled directly from the
    turn.sentence detector output. No narrative judgment.

Usage:
    python3 scripts/storyvideo.py --plan scripts/storyvideo_plan_2026-04-19.json
    python3 scripts/storyvideo.py 2026-04-19 --raw            # mechanical fallback
    python3 scripts/storyvideo.py --plan PLAN.json --out PATH

The plan JSON schema is documented in storyvideo_plan_v1.md (see scenes:
each is one of {title, quote, moment, outro} with template-specific fields).
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import urllib.request
from datetime import datetime, timedelta
from html import escape
from pathlib import Path

API = "http://localhost:3002"
DEFAULT_OUT = "scripts/recap-prototype/daystory-video/compositions/main-graphics.html"


# ─── Layer 1: facts (deterministic) ─────────────────────────────────────────


def fetch_sessions() -> list[dict]:
    with urllib.request.urlopen(f"{API}/api/sessions") as r:
        return json.loads(r.read()).get("sessions", [])


def fetch_sessionstory(sid: str) -> dict | None:
    try:
        out = subprocess.run(
            ["python3", "scripts/sessionstory.py", sid, "--json"],
            capture_output=True,
            text=True,
            check=True,
            timeout=30,
        )
        return json.loads(out.stdout)
    except (subprocess.CalledProcessError, subprocess.TimeoutExpired, json.JSONDecodeError):
        return None


def select_today_sessions(sessions: list[dict], date_str: str) -> list[dict]:
    day_start = datetime.fromisoformat(date_str + "T00:00:00")
    day_end = day_start + timedelta(days=1)
    today = []
    for s in sessions:
        try:
            start = datetime.fromisoformat(s["start_time"].replace("Z", ""))
            last = datetime.fromisoformat(s["last_event"].replace("Z", ""))
        except (KeyError, ValueError):
            continue
        if start < day_end and last >= day_start:
            today.append(s)
    today.sort(key=lambda s: s["start_time"])
    return today


def aggregate_tools(facts: list[dict]) -> list[tuple[str, int]]:
    totals: dict[str, int] = {}
    for f in facts:
        for tool, count in (f.get("tool_call_counts") or {}).items():
            totals[tool] = totals.get(tool, 0) + count
    return sorted(totals.items(), key=lambda x: -x[1])


def session_matches_query(fact: dict, terms: list[str]) -> bool:
    """A session matches if any query term appears (case-insensitive) in any
    of its sample sentences OR prompt timeline entries."""
    if not terms:
        return True
    haystack_parts = [s.lower() for s in fact.get("sample_sentences", [])]
    haystack_parts.extend(
        (p.get("content") or "").lower() for p in fact.get("prompt_timeline", [])
    )
    haystack = "\n".join(haystack_parts)
    return any(term in haystack for term in terms)


def collect_facts(date_str: str, query: str | None = None) -> dict:
    """Pull all the day's data and shape it for the renderer.

    If `query` is provided, sessions are filtered to those whose sentences
    or prompts contain any of the query terms (whitespace-split, lowercased).
    """
    sessions = fetch_sessions()
    today = select_today_sessions(sessions, date_str)
    facts_per_session = []
    for s in today:
        f = fetch_sessionstory(s["session_id"])
        if f and f.get("sample_sentences"):
            facts_per_session.append(f)

    terms = [t.lower() for t in (query or "").split() if len(t) > 2] if query else []
    if terms:
        before = len(facts_per_session)
        facts_per_session = [f for f in facts_per_session if session_matches_query(f, terms)]
        sys.stderr.write(
            f"  query {terms!r}: {before} → {len(facts_per_session)} sessions\n"
        )

    return {
        "date": date_str,
        "weekday_label": datetime.fromisoformat(date_str + "T00:00:00").strftime(
            "%A · %B %d, %Y"
        ),
        "query": query or "",
        "session_count": len(facts_per_session),
        "turn_count": sum(f.get("turn_count", 0) for f in facts_per_session),
        "record_count": sum(f.get("total_records", 0) for f in facts_per_session),
        "top_tools": aggregate_tools(facts_per_session)[:5],
        "raw_sessions": facts_per_session,
    }


# ─── Layer 2: planning (agentic) ────────────────────────────────────────────


def load_plan(path: str) -> dict:
    return json.loads(Path(path).read_text())


def raw_plan_from_facts(facts: dict, max_cards: int = 10) -> dict:
    """Fallback plan — mechanical sentence sampling. No narrative judgment.

    Useful for sanity-checking the renderer or for days where you don't want
    to author a plan.
    """
    raw = []
    for f in facts["raw_sessions"]:
        sid_short = f["session_id"][:8]
        time_label = (f.get("started_at") or "")[11:16]
        for sent in f.get("sample_sentences", []):
            raw.append((sid_short, time_label, sent))
    if len(raw) > max_cards:
        step = len(raw) / max_cards
        raw = [raw[int(i * step)] for i in range(max_cards)]

    scenes = [
        {
            "template": "title",
            "duration": 5,
            "headline_lines": [facts["weekday_label"]],
            "subtitle": f'{facts["session_count"]} sessions · {facts["turn_count"]} turns',
        }
    ]
    for sid, time_label, sent in raw:
        scenes.append(
            {
                "template": "quote",
                "duration": 4,
                "eyebrow": f"{time_label} // session {sid}",
                "text": sent,
                "tone": "neutral",
            }
        )
    scenes.append({"template": "outro", "duration": 4, "tagline": "raw — no narration"})
    return {
        "date": facts["date"],
        "weekday": facts["weekday_label"].split(" ·")[0],
        "headline": "raw sentence sampling",
        "scenes": scenes,
    }


# ─── Layer 3: render (deterministic) ────────────────────────────────────────


def scene_html(scene: dict, idx: int) -> str:
    """Render one scene as an HTML div based on its template."""
    template = scene["template"]
    tone = scene.get("tone", "neutral")

    if template == "title":
        lines = scene.get("headline_lines", [scene.get("headline", "Daystory")])
        sub = escape(scene.get("subtitle", ""))
        eyebrow = escape(scene.get("eyebrow", "▸ DAYSTORY"))
        line_html = "<br>".join(escape(l) for l in lines)
        return f"""    <div id="scene-{idx}" class="scene tpl-title">
      <div class="scene-inner">
        <div class="title-eyebrow">{eyebrow}</div>
        <h1 class="title-headline">{line_html}</h1>
        <div class="title-subtitle">{sub}</div>
      </div>
    </div>"""

    if template == "chapter":
        # Act/chapter break — large eyebrow label, optional subtitle
        label = escape(scene.get("label", "CHAPTER"))
        subtitle = escape(scene.get("subtitle", ""))
        sub_html = f'<div class="chapter-subtitle">{subtitle}</div>' if subtitle else ""
        return f"""    <div id="scene-{idx}" class="scene tpl-chapter">
      <div class="scene-inner">
        <div class="chapter-rule"></div>
        <div class="chapter-label">{label}</div>
        {sub_html}
      </div>
    </div>"""

    if template == "reflection":
        # Contemplative paragraph — italic, slower pace, voice
        text = escape(scene.get("text", ""))
        attr = escape(scene.get("attribution", ""))
        attr_html = f'<div class="refl-attr">— {attr}</div>' if attr else ""
        return f"""    <div id="scene-{idx}" class="scene tpl-reflection">
      <div class="scene-inner refl-inner">
        <div class="refl-mark">"</div>
        <p class="refl-text">{text}</p>
        {attr_html}
      </div>
    </div>"""

    if template == "quote":
        eyebrow = escape(scene.get("eyebrow", ""))
        text = escape(scene.get("text", ""))
        attr = escape(scene.get("attribution", ""))
        attr_html = f'<div class="quote-attr">— {attr}</div>' if attr else ""
        tone_class = f"tone-{tone}"
        return f"""    <div id="scene-{idx}" class="scene tpl-quote {tone_class}">
      <div class="scene-inner">
        <div class="quote-eyebrow">{eyebrow}</div>
        <p class="quote-text">{text}</p>
        {attr_html}
      </div>
    </div>"""

    if template == "moment":
        lines = scene.get("text_lines") or [scene.get("text", "")]
        sub = escape(scene.get("subtext", ""))
        sub_html = f'<div class="moment-sub">{sub}</div>' if sub else ""
        line_html = "<br>".join(escape(l) for l in lines)
        tone_class = f"tone-{tone}"
        return f"""    <div id="scene-{idx}" class="scene tpl-moment {tone_class}">
      <div class="scene-inner">
        <p class="moment-text">{line_html}</p>
        {sub_html}
      </div>
    </div>"""

    if template == "outro":
        tagline = escape(scene.get("tagline", ""))
        return f"""    <div id="scene-{idx}" class="scene tpl-outro">
      <div class="scene-inner">
        <div class="outro-mark">▸</div>
        <p class="outro-tagline">{tagline}</p>
      </div>
    </div>"""

    raise ValueError(f"unknown scene template: {template}")


def scene_timeline(scene: dict, idx: int, start: float) -> str:
    """GSAP timeline JS for one scene."""
    template = scene["template"]
    dur = float(scene["duration"])
    end = start + dur
    out_start = end - 0.45
    js = []
    js.append(
        f"      tl.to('#scene-{idx}', {{ opacity: 1, duration: 0.4 }}, {start});"
    )
    if template == "title":
        js.append(
            f"      tl.from('#scene-{idx} .title-eyebrow', {{ y: -12, opacity: 0, duration: 0.5, ease: 'power2.out' }}, {start + 0.2:.2f});"
        )
        js.append(
            f"      tl.from('#scene-{idx} .title-headline', {{ y: 30, opacity: 0, duration: 0.8, ease: 'power3.out' }}, {start + 0.4:.2f});"
        )
        js.append(
            f"      tl.from('#scene-{idx} .title-subtitle', {{ y: 16, opacity: 0, duration: 0.6, ease: 'expo.out' }}, {start + 0.85:.2f});"
        )
    elif template == "quote":
        js.append(
            f"      tl.from('#scene-{idx} .quote-eyebrow', {{ y: -10, opacity: 0, duration: 0.4, ease: 'power2.out' }}, {start + 0.2:.2f});"
        )
        js.append(
            f"      tl.from('#scene-{idx} .quote-text', {{ y: 22, opacity: 0, duration: 0.65, ease: 'power3.out' }}, {start + 0.35:.2f});"
        )
        if scene.get("attribution"):
            js.append(
                f"      tl.from('#scene-{idx} .quote-attr', {{ opacity: 0, duration: 0.4, ease: 'power1.out' }}, {start + 0.85:.2f});"
            )
    elif template == "moment":
        js.append(
            f"      tl.from('#scene-{idx} .moment-text', {{ scale: 0.88, opacity: 0, duration: 0.55, ease: 'power4.out' }}, {start + 0.15:.2f});"
        )
        if scene.get("subtext"):
            js.append(
                f"      tl.from('#scene-{idx} .moment-sub', {{ y: 14, opacity: 0, duration: 0.5, ease: 'power2.out' }}, {start + 0.6:.2f});"
            )
    elif template == "chapter":
        js.append(
            f"      tl.from('#scene-{idx} .chapter-rule', {{ scaleX: 0, transformOrigin: 'center', duration: 0.5, ease: 'expo.out' }}, {start + 0.15:.2f});"
        )
        js.append(
            f"      tl.from('#scene-{idx} .chapter-label', {{ y: 24, opacity: 0, duration: 0.65, ease: 'power3.out' }}, {start + 0.35:.2f});"
        )
        if scene.get("subtitle"):
            js.append(
                f"      tl.from('#scene-{idx} .chapter-subtitle', {{ y: 12, opacity: 0, duration: 0.55, ease: 'power2.out' }}, {start + 0.7:.2f});"
            )
    elif template == "reflection":
        js.append(
            f"      tl.from('#scene-{idx} .refl-mark', {{ opacity: 0, scale: 0.6, duration: 0.6, ease: 'power2.out' }}, {start + 0.2:.2f});"
        )
        js.append(
            f"      tl.from('#scene-{idx} .refl-text', {{ y: 28, opacity: 0, duration: 0.95, ease: 'power3.out' }}, {start + 0.45:.2f});"
        )
        if scene.get("attribution"):
            js.append(
                f"      tl.from('#scene-{idx} .refl-attr', {{ opacity: 0, duration: 0.5, ease: 'power1.out' }}, {start + 1.2:.2f});"
            )
    elif template == "outro":
        js.append(
            f"      tl.from('#scene-{idx} .outro-mark', {{ scale: 0, opacity: 0, duration: 0.5, ease: 'back.out(1.5)' }}, {start + 0.2:.2f});"
        )
        js.append(
            f"      tl.from('#scene-{idx} .outro-tagline', {{ y: 16, opacity: 0, duration: 0.7, ease: 'power3.out' }}, {start + 0.5:.2f});"
        )
    js.append(
        f"      tl.to('#scene-{idx}', {{ opacity: 0, duration: 0.45 }}, {out_start:.2f});"
    )
    return "\n".join(js)


def build_composition(plan: dict, facts: dict) -> str:
    scenes = plan["scenes"]
    starts = []
    cursor = 0.0
    for sc in scenes:
        starts.append(cursor)
        cursor += float(sc["duration"])
    total = cursor + 0.6  # tail black-out

    scene_divs = "\n".join(scene_html(sc, i) for i, sc in enumerate(scenes))
    scene_js = "\n".join(scene_timeline(sc, i, starts[i]) for i, sc in enumerate(scenes))

    # Persistent footer (always-on day metadata)
    tool_chips = " ".join(
        f'<span class="chip"><span class="chip-name">{escape(t)}</span> '
        f'<span class="chip-count">{c}</span></span>'
        for t, c in facts.get("top_tools", [])
    )
    query_chip = (
        f'<span class="ftr-query">query: <b>{escape(facts.get("query", ""))}</b></span>'
        f'<span class="ftr-dot">·</span>'
        if facts.get("query")
        else ""
    )
    footer = f"""    <div id="footer">
      <span class="ftr-mark">▸</span>
      <span class="ftr-meta">openstory · {escape(facts["weekday_label"])}</span>
      <span class="ftr-dot">·</span>
      {query_chip}
      <span class="ftr-stat"><b>{facts["session_count"]}</b> sessions</span>
      <span class="ftr-dot">·</span>
      <span class="ftr-stat"><b>{facts["turn_count"]}</b> turns</span>
      <span class="ftr-dot">·</span>
      <span class="ftr-stat"><b>{facts["record_count"]:,}</b> records</span>
    </div>"""

    return f"""<template id="main-graphics-template">
  <div
    data-composition-id="main-graphics"
    data-width="1920"
    data-height="1080"
    data-start="0"
    data-duration="{total:.1f}"
  >
{footer}

{scene_divs}

    <div id="black-out"></div>

    <style>
      [data-composition-id="main-graphics"] {{
        position: relative;
        width: 100%;
        height: 100%;
        background: #0a0e14;
        color: #e6e6e6;
        font-family: "Inter", sans-serif;
        overflow: hidden;
      }}

      /* Persistent footer — day identity, always visible */
      #footer {{
        position: absolute;
        bottom: 60px;
        left: 100px;
        right: 100px;
        font-family: "JetBrains Mono", monospace;
        font-size: 19px;
        color: #565f70;
        display: flex;
        align-items: baseline;
        gap: 14px;
        opacity: 0;
        z-index: 25;
      }}
      #footer .ftr-mark {{ color: #ffaa3b; }}
      #footer .ftr-meta {{ color: #8a9199; margin-right: 6px; }}
      #footer .ftr-dot {{ color: #1f2630; }}
      #footer .ftr-stat b {{ color: #e6e6e6; font-weight: 700; }}
      #footer .chip {{ padding: 2px 10px; background: #11161e; border: 1px solid #1f2630; border-radius: 2px; }}
      #footer .chip-count {{ color: #ffaa3b; }}
      #footer .ftr-query {{ color: #8a9199; }}
      #footer .ftr-query b {{ color: #ffaa3b; font-weight: 700; }}

      /* Scene base */
      .scene {{
        position: absolute;
        top: 0; left: 0;
        width: 100%;
        height: 100%;
        padding: 140px 200px 200px;
        opacity: 0;
        z-index: 1;
      }}
      .scene-inner {{
        width: 100%;
        height: 100%;
        display: flex;
        flex-direction: column;
        justify-content: center;
        align-items: center;
        text-align: center;
        gap: 28px;
      }}

      /* tpl-title */
      .tpl-title .title-eyebrow {{
        font-family: "JetBrains Mono", monospace;
        font-size: 26px;
        color: #ffaa3b;
        letter-spacing: 0.3em;
      }}
      .tpl-title .title-headline {{
        font-family: "JetBrains Mono", monospace;
        font-size: 96px;
        font-weight: 700;
        line-height: 1.15;
        letter-spacing: -0.01em;
        max-width: 1500px;
        margin: 0;
      }}
      .tpl-title .title-subtitle {{
        font-family: "Inter", sans-serif;
        font-size: 36px;
        color: #8a9199;
        letter-spacing: 0.04em;
      }}

      /* tpl-quote */
      .tpl-quote .quote-eyebrow {{
        font-family: "JetBrains Mono", monospace;
        font-size: 22px;
        color: #ffaa3b;
        letter-spacing: 0.25em;
      }}
      .tpl-quote .quote-text {{
        font-family: "Inter", sans-serif;
        font-size: 64px;
        font-weight: 400;
        line-height: 1.3;
        max-width: 1500px;
        margin: 0;
      }}
      .tpl-quote .quote-attr {{
        font-family: "JetBrains Mono", monospace;
        font-size: 22px;
        color: #565f70;
        margin-top: 8px;
      }}
      /* Tones — color tweaks per quote scene */
      .tpl-quote.tone-neutral .quote-text {{ color: #e6e6e6; }}
      .tpl-quote.tone-warm .quote-eyebrow {{ color: #ffd28a; }}
      .tpl-quote.tone-warm .quote-text {{ color: #f3e7d3; font-style: italic; }}
      .tpl-quote.tone-warm .quote-attr {{ color: #ffaa3b; }}

      /* tpl-chapter — act break / section marker */
      .tpl-chapter .scene-inner {{ gap: 32px; }}
      .tpl-chapter .chapter-rule {{
        width: 280px;
        height: 1px;
        background: #ffaa3b;
        opacity: 0.7;
      }}
      .tpl-chapter .chapter-label {{
        font-family: "JetBrains Mono", monospace;
        font-size: 88px;
        font-weight: 700;
        color: #ffaa3b;
        letter-spacing: 0.18em;
        text-transform: uppercase;
        margin: 0;
      }}
      .tpl-chapter .chapter-subtitle {{
        font-family: "Inter", sans-serif;
        font-size: 32px;
        color: #8a9199;
        font-style: italic;
        max-width: 1300px;
      }}

      /* tpl-reflection — contemplative paragraph, italic */
      .tpl-reflection .refl-inner {{ gap: 24px; }}
      .tpl-reflection .refl-mark {{
        font-family: "Inter", serif;
        font-size: 180px;
        color: #ffaa3b;
        line-height: 0.6;
        opacity: 0.55;
        margin-bottom: 12px;
      }}
      .tpl-reflection .refl-text {{
        font-family: "Inter", sans-serif;
        font-size: 48px;
        font-style: italic;
        font-weight: 400;
        line-height: 1.45;
        color: #e6e6e6;
        max-width: 1450px;
        margin: 0;
        text-align: center;
      }}
      .tpl-reflection .refl-attr {{
        font-family: "JetBrains Mono", monospace;
        font-size: 22px;
        color: #8a9199;
        letter-spacing: 0.04em;
        margin-top: 16px;
      }}

      /* tpl-moment — energy / resistance beats */
      .tpl-moment .moment-text {{
        font-family: "JetBrains Mono", monospace;
        font-size: 240px;
        font-weight: 700;
        line-height: 1.05;
        margin: 0;
        letter-spacing: -0.02em;
      }}
      .tpl-moment .moment-sub {{
        font-family: "JetBrains Mono", monospace;
        font-size: 36px;
        color: #8a9199;
        letter-spacing: 0.06em;
      }}
      .tpl-moment.tone-positive .moment-text {{ color: #ffaa3b; }}
      .tpl-moment.tone-positive .moment-sub {{ color: #ffd28a; }}
      .tpl-moment.tone-negative .moment-text {{ color: #d35454; font-size: 170px; line-height: 1.15; }}
      .tpl-moment.tone-negative .moment-sub {{ color: #b06868; }}
      .tpl-moment.tone-neutral .moment-text {{ color: #e6e6e6; }}

      /* tpl-outro */
      .tpl-outro .outro-mark {{
        font-family: "JetBrains Mono", monospace;
        font-size: 100px;
        color: #ffaa3b;
        line-height: 1;
      }}
      .tpl-outro .outro-tagline {{
        font-family: "Inter", sans-serif;
        font-size: 56px;
        font-weight: 400;
        color: #e6e6e6;
        font-style: italic;
        max-width: 1300px;
        margin: 0;
      }}

      /* Black out */
      #black-out {{
        position: absolute;
        top: 0; left: 0;
        width: 100%;
        height: 100%;
        background: #0a0e14;
        opacity: 0;
        z-index: 100;
      }}
    </style>

    <script src="https://cdn.jsdelivr.net/npm/gsap@3.14.2/dist/gsap.min.js"></script>
    <script>
      const tl = gsap.timeline({{ paused: true }});

      // Persistent footer fades in once
      tl.to('#footer', {{ opacity: 1, duration: 0.7, ease: 'power2.out' }}, 0.3);

      // Scenes
{scene_js}

      // Final fade
      tl.to('#footer', {{ opacity: 0, duration: 0.5 }}, {total - 0.6:.2f});
      tl.to('#black-out', {{ opacity: 1, duration: 0.4 }}, {total - 0.4:.2f});

      window.__timelines = window.__timelines || {{}};
      window.__timelines['main-graphics'] = tl;
    </script>
  </div>
</template>
"""


# ─── CLI ────────────────────────────────────────────────────────────────────


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("date", nargs="?", default=datetime.utcnow().strftime("%Y-%m-%d"))
    ap.add_argument("--plan", help="Path to scene plan JSON")
    ap.add_argument("--raw", action="store_true", help="Mechanical sentence sampling (no narrative)")
    ap.add_argument("--print-facts", action="store_true",
                    help="Emit collected facts as JSON to stdout and exit (for skill/model consumption)")
    ap.add_argument("--query",
                    help='Filter sessions to those mentioning these terms (whitespace-split). '
                         'e.g. --query "video hyperframes" — for thread-of-work narration.')
    ap.add_argument("--out", default=DEFAULT_OUT)
    ap.add_argument("--max-cards", type=int, default=10, help="Cards in --raw mode")
    args = ap.parse_args()

    if args.print_facts:
        facts = collect_facts(args.date, query=args.query)
        slim = {
            "date": facts["date"],
            "weekday_label": facts["weekday_label"],
            "session_count": facts["session_count"],
            "turn_count": facts["turn_count"],
            "record_count": facts["record_count"],
            "top_tools": facts["top_tools"],
            "sessions": [
                {
                    "session_id": s["session_id"],
                    "started_at": s.get("started_at", ""),
                    "duration_hours": s.get("duration_hours", 0),
                    "turn_count": s.get("turn_count", 0),
                    "tool_call_counts": s.get("tool_call_counts", {}),
                    "sample_sentences": s.get("sample_sentences", []),
                    "prompt_timeline": s.get("prompt_timeline", []),
                }
                for s in facts["raw_sessions"]
            ],
        }
        json.dump(slim, sys.stdout, indent=2)
        sys.stdout.write("\n")
        return 0

    if not args.plan and not args.raw:
        ap.error("must pass --plan PATH, --raw, or --print-facts")

    label = f"workstory ({args.query!r})" if args.query else "daystory"
    print(f"{label} video for {args.date}")
    print("  collecting facts ...")
    facts = collect_facts(args.date, query=args.query)
    print(f"  {facts['session_count']} sessions, {facts['turn_count']} turns, "
          f"{facts['record_count']:,} records")
    if facts["session_count"] == 0:
        print("  no sessions matched — exiting")
        return 1

    if args.plan:
        plan = load_plan(args.plan)
        print(f"  using plan: {args.plan} ({len(plan['scenes'])} scenes)")
    else:
        plan = raw_plan_from_facts(facts, args.max_cards)
        print(f"  raw plan: {len(plan['scenes'])} scenes (1 title + {len(plan['scenes']) - 2} cards + outro)")

    html = build_composition(plan, facts)
    out_path = Path(args.out)
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(html)
    duration = sum(float(s["duration"]) for s in plan["scenes"])
    print(f"  wrote {out_path} (~{duration:.0f}s composition)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
