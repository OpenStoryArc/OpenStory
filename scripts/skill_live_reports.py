"""Drive each OpenStory skill against the REST API and render an HTML report each.

This exercises the REST-only data path (the exact endpoints the REST MCP wraps),
proving every skill works without touching SQLite directly. One /tmp file per skill,
styled with the shared chart system (Georgetown theme).

    python3 scripts/skill_live_reports.py        # writes + prints /tmp/skill-*.html
"""

import html
import json
import sys
import urllib.parse
import urllib.request
from datetime import date

sys.path.insert(0, "/Users/maxglassie/projects/OpenStory/scripts")
from build_prompt_library import _CSS, _block, root_css  # noqa: E402

API = "http://localhost:3002"
THEME = "georgetown"


def get(path):
    with urllib.request.urlopen(API + path, timeout=20) as r:
        return json.loads(r.read())


_PAGE = """<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>/{skill} — live REST report</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Public+Sans:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500;600&family=Castoro:ital@0;1&display=swap" rel="stylesheet">
<style>{css}</style></head>
<body><div class="wrap">
  <header>
    <p class="eyebrow">OpenStory · /openstory:{skill} · live over REST</p>
    <h1>{h1}</h1>
    <p class="sub">{sub}</p>
  </header>
  <section class="{ac}"><div class="viz">{blocks}<div class="src">{src}</div></div></section>
  <footer>Driven against http://localhost:3002 — REST only, zero direct SQLite.</footer>
</div></body></html>"""


def page(skill, ac, h1, sub, blocks, src):
    css = root_css(THEME) + _CSS
    return _PAGE.format(css=css, skill=skill, ac=ac, h1=html.escape(h1),
                        sub=html.escape(sub), src=html.escape(src),
                        blocks="".join(_block(b) for b in blocks))


# -- one builder per skill (returns the page tuple) -------------------

def cost_report():
    t = get("/api/insights/token-usage?days=7&model=opus")
    daily = get("/api/insights/token-usage/daily?days=7")
    c = t["cost"]
    total = c["total"]
    in_rate = (c["input"] / t["input_tokens"]) if t["input_tokens"] else 0
    retail = c["input"] + c["output"] + (t["cache_read_tokens"] + t["cache_creation_tokens"]) * in_rate
    saved = max(0.0, retail - total)
    pct = round(saved / retail * 100) if retail else 0
    stats = [{"num": f"${total:,.0f}", "label": f"spent · 7d ({c.get('model', '')})"},
             {"num": f"${saved:,.0f}", "label": "saved by cache", "accent": True},
             {"num": f"{pct}%", "label": "off retail", "accent": True},
             {"num": str(t["session_count"]), "label": "sessions"}]
    mx = max((d["total_tokens"] for d in daily), default=1)
    bars = [{"label": d["date"][5:], "bar": round(d["total_tokens"] / mx * 100),
             "val": f"{d['total_tokens'] / 1e6:.0f}M"} for d in daily]
    blocks = [{"type": "stats", "items": stats},
              {"type": "bars", "title": "Tokens per day", "items": bars},
              {"type": "note", "text": f"**{t['message_count']:,}** messages across the window."}]
    return ("cost", "blue", "What have my agent sessions cost?",
            "Total spend + tokens-per-day, pulled over REST.", blocks,
            "GET /api/insights/token-usage  +  /daily")


def recall_report():
    hits = get("/api/search?q=tailscale")
    sessions = {}
    for h in hits:
        sessions[h["session_id"]] = sessions.get(h["session_id"], 0) + 1
    items = [{"date": h["record_type"][:12], "label": " ".join(h["snippet"].split())[:88],
              "meta": h["session_id"][:8]} for h in hits[:8]]
    blocks = [{"type": "stats", "items": [{"num": str(len(hits)), "label": "matches", "accent": True},
                                          {"num": str(len(sessions)), "label": "sessions"}]},
              {"type": "list", "items": items},
              {"type": "note", "text": 'Full-text search for **"tailscale"** across every session (FTS, over REST).'}]
    return ("recall", "orange", 'Recall: where did I touch "tailscale"?',
            "Associative memory across all sessions, over REST.", blocks,
            "GET /api/search?q=tailscale")


def recap_report():
    pulse = sorted(get("/api/insights/pulse?days=7"), key=lambda p: p["event_count"], reverse=True)
    tot_s = sum(p["session_count"] for p in pulse)
    tot_e = sum(p["event_count"] for p in pulse)
    mx = max((p["event_count"] for p in pulse), default=1)
    rows = [[p["project_name"] or "(none)", str(p["session_count"]),
             {"bar": round(p["event_count"] / mx * 100), "text": f"{p['event_count'] / 1000:.1f}K"}]
            for p in pulse]
    blocks = [{"type": "stats", "items": [{"num": str(len(pulse)), "label": "projects"},
                                          {"num": str(tot_s), "label": "sessions"},
                                          {"num": f"{tot_e / 1000:.1f}K", "label": "events", "accent": True}]},
              {"type": "table", "cols": ["Project", "Sessions", "Activity"], "align": ["l", "r", "l"], "rows": rows}]
    return ("recap", "cyan", "What have I worked on this week?",
            "Activity grouped by project, over REST.", blocks,
            "GET /api/insights/pulse?days=7")


def standup_report():
    sessions = get("/api/sessions")["sessions"]
    today = date.today().isoformat()
    todays = [s for s in sessions if (s.get("last_event") or "").startswith(today)]
    did = [f'{(s["label"] or "(no label)")[:58]} · {s.get("project_name") or "?"} ({s["event_count"]} ev)'
           for s in todays[:6]] or ["(no sessions yet today)"]
    blocks = [{"type": "stats", "items": [{"num": str(len(todays)), "label": "sessions today", "accent": True}]},
              {"type": "groups", "cols": [
                  {"title": "Did (today's threads)", "items": did},
                  {"title": "Blocked", "tone": "warn", "items": ["infer from unresolved errors"]},
                  {"title": "Next", "items": ["infer from open threads"]}]},
              {"type": "note", "text": "Synthesized from today's sessions; Did is grounded in real session labels, Blocked/Next want model inference on session detail."}]
    return ("standup", "purple", "Standup — today",
            "Today's sessions → Did / Blocked / Next, over REST.", blocks,
            "GET /api/sessions  (filtered to today)")


def coach_report():
    prod = {p["hour"]: p["event_count"] for p in get("/api/insights/productivity?days=30")}
    mx = max(prod.values(), default=1)
    bars = [{"label": f"{h:02d}:00", "bar": round(prod.get(h, 0) / mx * 100), "val": str(prod.get(h, 0))}
            for h in range(24) if prod.get(h, 0) > 0]
    peak = max(prod, key=prod.get) if prod else 0
    blocks = [{"type": "bars", "title": "When you work — events by hour (30d, UTC)", "items": bars},
              {"type": "note", "text": f"Peak hour: **{peak:02d}:00 UTC**. One signal of several — full coaching also folds in failure patterns and error rates."}]
    return ("coach", "green", "Coach: how do I work?",
            "Your rhythm, over REST (one lens of the full scorecard).", blocks,
            "GET /api/insights/productivity?days=30")


def scan_report():
    cats = [("Inline credentials", ["password", "secret", "api_key"], "High"),
            ("Key prefixes", ["sk-", "ghp_", "AKIA"], "High"),
            ("Tokens / bearer", ["Bearer", "token"], "Medium"),
            ("Email addresses", ["gmail", "proton"], "Medium")]
    rows = []
    for name, terms, sev in cats:
        hits, sids = 0, set()
        for term in terms:
            try:
                res = get("/api/search?q=" + urllib.parse.quote(term))
                hits += len(res)
                sids.update(r["session_id"] for r in res)
            except Exception:
                pass
        rows.append([name, str(hits), str(len(sids)), {"chip": sev}])
    blocks = [{"type": "stats", "items": [{"num": "redacted", "label": "counts only — no values", "accent": True}]},
              {"type": "table", "cols": ["Category", "Hits", "Sessions", "Severity"],
               "align": ["l", "r", "r", "r"], "rows": rows},
              {"type": "note", "tone": "warn", "text": "**No secret values printed** — counts only. Heuristic FTS scan over REST; a server-side regex sweep would be thorough."}]
    return ("scan", "red", "Scan: anything sensitive before I share?",
            "Pre-share safety check, over REST — redacted summary only.", blocks,
            "GET /api/search?q=<pattern>  (per category)")


if __name__ == "__main__":
    builders = [cost_report, recall_report, recap_report, standup_report, coach_report, scan_report]
    out = []
    for fn in builders:
        try:
            skill, ac, h1, sub, blocks, src = fn()
            path = f"/tmp/skill-{skill}.html"
            with open(path, "w", encoding="utf-8") as f:
                f.write(page(skill, ac, h1, sub, blocks, src))
            out.append(path)
            print(f"  OK   {path}")
        except Exception as e:
            print(f"  FAIL {fn.__name__}: {e}")
    print("\n".join(out))
