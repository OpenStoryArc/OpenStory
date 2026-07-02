"""Archetype charts as Vega-Lite specs — typed + cited, all the way into the viz.

A Vega-Lite spec is a JSON document, exactly like our cited schemas. So the
citations ride along in the spec's `usermeta.x-citation`, and the chart carries
its own research roots. Altair (the Python binding) lets the SAME pure functions
that compute the readings emit the chart — no hand-written JS, no build step.

    python3 scripts/archetype_charts.py --days 30 --sample 60   # → /tmp + open
    python3 scripts/archetype_charts.py --out scripts/.dist.html --no-open
    python3 scripts/archetype_charts.py --test

Outputs two artifacts: the self-contained HTML report, and the cited Vega-Lite
spec (`*.vl.json`) — the typed, cited chart document on its own.
"""

import argparse
import json
import statistics
import subprocess
import sys
from pathlib import Path

import altair as alt

import session_distribution as sd
import metric_trace as mt
import cited_schema as cs

INK, MUTED, LINE, PANEL, BG = "#e8edf2", "#7d8a99", "#222a33", "#12161c", "#0b0d10"
ACCENT, WARN = "#5ad1c5", "#f2b15a"


def _citations() -> dict:
    """Resolve the chart's research roots from the registry — for usermeta."""
    reg = cs.load_registry()
    b = mt.PIPELINE_BACKING
    return {
        "x-axis (net_lean)": cs.cite(reg, b["the_axis"]["theory"], b["the_axis"]["strength"], b["the_axis"]["note"]),
        "the unit (one point = one session)": cs.cite(reg, b["process_locus"]["theory"],
            b["process_locus"]["strength"], b["process_locus"]["note"]),
        "showing a distribution, not one number": cs.cite(reg, "dietrich-2004", b["distribution"]["strength"],
            b["distribution"]["note"]),
        "median rule (not the volume-weighted pool)": cs.cite(reg, "space-2021", "validated-frame",
            "the honest summary — median session, not the volume-weighted pool"),
    }


def distribution_spec(readings: list[dict]) -> dict:
    """Build the cited Vega-Lite spec for the per-session distribution."""
    leans = [r["net_lean"] for r in readings]
    median = round(statistics.median(leans), 3)
    rows = [{"net_lean": r["net_lean"], "project": r["project"],
             "label": r["label"] or r["sid"][:8], "events": r["events"]} for r in readings]

    points = (
        alt.Chart(alt.InlineData(values=rows))
        .mark_circle(opacity=0.75, stroke=BG, strokeWidth=0.5)
        .encode(
            x=alt.X("net_lean:Q", scale=alt.Scale(domain=[-1, 1]),
                    axis=alt.Axis(title="←  spontaneous (associative)        ·        deliberate (controlled)  →",
                                  values=[-1, -0.5, 0, 0.5, 1])),
            y=alt.Y("project:N", title=None, sort="-x"),
            size=alt.Size("events:Q", title="events", scale=alt.Scale(range=[30, 600])),
            color=alt.Color("project:N", legend=None,
                            scale=alt.Scale(scheme="tableau20")),
            tooltip=[alt.Tooltip("label:N", title="session"), alt.Tooltip("project:N"),
                     alt.Tooltip("net_lean:Q", format="+.2f"), alt.Tooltip("events:Q")],
        )
    )
    rule = (alt.Chart(alt.InlineData(values=[{"m": median}]))
            .mark_rule(color=WARN, strokeDash=[5, 4], size=1.5)
            .encode(x="m:Q"))
    chart = (rule + points).properties(
        width=720, height=max(180, 26 * len({r["project"] for r in rows})),
        title=alt.TitleParams(
            text=f"Session distribution · deliberate ↔ spontaneous   (median {median:+.2f}, n={len(rows)})",
            color=INK, fontSize=15, anchor="start"),
    ).configure(background=PANEL).configure_view(stroke=None).configure_axis(
        labelColor=MUTED, titleColor=MUTED, gridColor=LINE, domainColor=LINE, tickColor=LINE,
    )

    spec = chart.to_dict()
    # The whole point: citations travel inside the spec, not beside it.
    spec["usermeta"] = {"x-citation": _citations(),
                        "summary": {"median": median, "n": len(rows),
                                    "event_weighted": round(
                                        sum(r["net_lean"] * r["events"] for r in readings)
                                        / max(1, sum(r["events"] for r in readings)), 3)}}
    return spec


# ── Self-contained HTML around the cited spec ─────────────────────────────────
def render_html(spec: dict) -> str:
    um = spec.get("usermeta", {})
    cites = um.get("x-citation", {})

    def row(k, c):
        src = c.get("primary_source")
        root = f'<a href="{src}" target="_blank">{src}</a>' if src else "⚠ uncited"
        return (f'<div class="c"><div class="ck">{k}</div>'
                f'<div class="cw">{c["warrant"]} '
                f'<span class="bdg {c["warrant_strength"]}">{c["warrant_strength"]}</span></div>'
                f'<div class="cr">{root}</div></div>')

    rows = "".join(row(k, c) for k, c in cites.items())
    s = um.get("summary", {})
    return _TPL.replace("/*SPEC*/", json.dumps(spec)).replace("<!--CITES-->", rows).replace(
        "MEDIAN", f'{s.get("median", 0):+.2f}').replace("POOLED", f'{s.get("event_weighted", 0):+.2f}')


_TPL = r"""<!doctype html><html><head><meta charset="utf-8"/>
<title>Open Story · Archetype Distribution</title>
<script src="https://cdn.jsdelivr.net/npm/vega@5"></script>
<script src="https://cdn.jsdelivr.net/npm/vega-lite@5"></script>
<script src="https://cdn.jsdelivr.net/npm/vega-embed@6"></script>
<style>
:root{--bg:#0b0d10;--panel:#12161c;--line:#222a33;--ink:#e8edf2;--muted:#7d8a99;--accent:#5ad1c5;--warn:#f2b15a;--bad:#e0667a;--good:#7ee0a8;--mono:ui-monospace,Menlo,Consolas,monospace}
body{margin:0;background:var(--bg);color:var(--ink);font-family:var(--mono)}
.wrap{max-width:860px;margin:0 auto;padding:40px 24px 80px}
.brand{letter-spacing:.18em;font-size:12px;color:var(--muted);text-transform:uppercase}
.local{font-size:11px;color:var(--accent);border:1px solid var(--accent);border-radius:999px;padding:3px 10px;margin-left:8px}
.panel{background:var(--panel);border:1px solid var(--line);border-radius:14px;padding:20px;margin-top:22px}
h2{font-size:11px;letter-spacing:.16em;text-transform:uppercase;color:var(--muted);font-weight:600;margin:0 0 12px}
.sub{color:var(--muted);font-size:13px}.sub b{color:var(--ink)}
#vis{margin:6px 0}
.c{border-top:1px solid var(--line);padding:10px 0;font-size:12.5px}.c:first-child{border-top:0}
.ck{color:var(--ink)}.cw{color:var(--muted);margin-top:2px}.cr{color:var(--muted);margin-top:2px;font-size:12px}
a{color:var(--accent);text-decoration:none}a:hover{text-decoration:underline}
.bdg{font-size:10px;text-transform:uppercase;border-radius:999px;padding:1px 7px;border:1px solid}
.bdg.validated,.bdg.validated-frame{color:var(--good);border-color:var(--good)}
.bdg.analogical{color:var(--warn);border-color:var(--warn)}
.bdg.metaphorical,.bdg.none{color:var(--bad);border-color:var(--bad)}
</style></head><body><div class="wrap">
<div><span class="brand">Open Story · Archetype Distribution</span><span class="local">local-only</span></div>
<div class="panel">
  <div id="vis"></div>
  <div class="sub">Each point is one session (size = events). The dashed line is the <b>median (MEDIAN)</b> — the honest summary. The volume-weighted pool (POOLED) is dragged toward zero by a few giant sessions, which is why we don't report it as the number.</div>
</div>
<div class="panel">
  <h2>The chart's citations · carried in the Vega-Lite spec's <code>usermeta.x-citation</code></h2>
  <!--CITES-->
</div>
</div>
<script>const SPEC=/*SPEC*/;vegaEmbed('#vis',SPEC,{actions:{export:true,source:true,editor:false}});</script>
</body></html>"""


def run_tests() -> int:
    rows = [{"net_lean": -0.4, "project": "A", "label": "x", "events": 100, "sid": "a1b2c3d4"},
            {"net_lean": 0.1, "project": "B", "label": "y", "events": 50, "sid": "b2c3d4e5"}]
    spec = distribution_spec(rows)
    ok = ("usermeta" in spec and "x-citation" in spec["usermeta"]
          and any("dietrich" in (c.get("theory_id") or "") for c in spec["usermeta"]["x-citation"].values())
          and "mark" in json.dumps(spec) and "/*SPEC*/" not in render_html(spec))
    print("ok — archetype-charts test passed" if ok else "FAIL"); return 0 if ok else 1


def main() -> int:
    ap = argparse.ArgumentParser(description="Cited Vega-Lite archetype charts (Altair)")
    ap.add_argument("--days", type=int, default=30)
    ap.add_argument("--sample", type=int, default=60)
    ap.add_argument("--out", help="HTML output path (default /tmp/archetype-distribution.html)")
    ap.add_argument("--no-open", action="store_true")
    ap.add_argument("--api", default=sd.pd.DEFAULT_API)
    ap.add_argument("--test", action="store_true")
    args = ap.parse_args()
    if args.test:
        return run_tests()

    readings = sd.distribution(args.api, args.days, args.sample)
    if not readings:
        print("no sessions in window"); return 1
    spec = distribution_spec(readings)
    out = Path(args.out) if args.out else Path("/tmp/archetype-distribution.html")
    out.write_text(render_html(spec))
    spec_path = out.with_suffix(".vl.json")
    spec_path.write_text(json.dumps(spec, indent=2, ensure_ascii=False))
    print(f"wrote {out}\n      {spec_path}  (cited Vega-Lite spec · {len(readings)} sessions)")
    if not args.no_open:
        subprocess.run(["open", str(out)], check=False)
    return 0


if __name__ == "__main__":
    sys.exit(main())
