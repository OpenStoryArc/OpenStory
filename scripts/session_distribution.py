"""Per-session distribution of the deliberate↔spontaneous reading — bottom-up.

Built from the single-session primitive (`metric_trace.session_reading`): we
compute ONE reading per session and look at the spread, instead of pooling
everything into a single volume-weighted number. The question this answers:
do sessions of different KINDS land in different places on the axis? If yes,
"overall" is the wrong unit and segmentation is real. If they all bunch up,
the pooled number was fine.

    python3 scripts/session_distribution.py                 # distribution, last 30d
    python3 scripts/session_distribution.py --session <SID> # the single primitive
    python3 scripts/session_distribution.py --by project     # group by project
    python3 scripts/session_distribution.py --test

Bottom-up by design: `--session` shows the atom; the default composes atoms into
a distribution. The aggregate is never a separate computation — just many atoms.
"""

import argparse
import statistics
import sys
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta, timezone

import profile_dimensions as pd
import metric_trace as mt


def short_project(p: str) -> str:
    """'-Users-maxglassie-projects-OpenStory' → 'OpenStory'."""
    if not p:
        return "?"
    return p.rstrip("-").split("-")[-1] or p


def reading_for(api: str, sess: dict) -> dict:
    """Compose: fetch one session's journey, run the primitive, attach metadata."""
    sid = sess["session_id"]
    journey = pd._get(api, f"/api/sessions/{sid}/tool-journey") or []
    steps = mt.steps_from_journey(journey if isinstance(journey, list) else [])
    r = mt.session_reading(steps)
    return {
        "sid": sid, "net_lean": r["net_lean"], "signals": r["signals"],
        "events": sess.get("event_count", 0),
        "project": short_project(sess.get("project_name") or sess.get("project_id") or "?"),
        "label": (sess.get("label") or "").strip()[:42],
    }


def distribution(api: str, days: int, sample: int, min_events: int = 30) -> list[dict]:
    cutoff = datetime.now(timezone.utc) - timedelta(days=days)
    sessions = (pd._get(api, "/api/sessions") or {}).get("sessions", [])
    sessions = [s for s in sessions
                if pd._within(s.get("last_event", ""), cutoff)
                and s.get("event_count", 0) >= min_events]
    top = sorted(sessions, key=lambda s: s.get("event_count", 0), reverse=True)[:sample]
    with ThreadPoolExecutor(max_workers=8) as pool:
        return list(pool.map(lambda s: reading_for(api, s), top))


# ── Rendering ────────────────────────────────────────────────────────────────
def axis_cell(net: float, width: int = 33) -> str:
    """Place a session on a spontaneous←→deliberate strip."""
    mid = width // 2
    pos = max(0, min(width - 1, int(round(mid + net * mid))))
    cells = ["·"] * width
    cells[mid] = "|"
    cells[pos] = "●"
    return "".join(cells)


def render(readings: list[dict], by: str | None) -> str:
    if not readings:
        return "no sessions in window."
    leans = [r["net_lean"] for r in readings]
    L = []
    L.append("PER-SESSION DISTRIBUTION · deliberate ↔ spontaneous")
    L.append("=" * 78)
    L.append("spont ← " + " " * 9 + "→ delib   lean   ev    project / label")
    L.append("-" * 78)
    for r in sorted(readings, key=lambda r: r["net_lean"]):
        L.append(f"{axis_cell(r['net_lean'])}  {r['net_lean']:+.2f}  {r['events']:>4}  "
                 f"{r['project']:<11} {r['label']}")
    L.append("-" * 78)
    # The punchline: one pooled number vs the actual spread.
    ev_weighted = sum(r["net_lean"] * r["events"] for r in readings) / sum(r["events"] for r in readings)
    L.append(f"sessions: {len(readings)}    "
             f"median {statistics.median(leans):+.2f}    "
             f"mean {statistics.mean(leans):+.2f}    "
             f"range {min(leans):+.2f}…{max(leans):+.2f}    "
             f"spread {max(leans)-min(leans):.2f}")
    L.append(f"event-weighted (≈ the report's single number): {ev_weighted:+.2f}")

    if by == "project":
        L.append("")
        L.append("BY PROJECT (mean lean · n sessions):")
        groups: dict[str, list] = {}
        for r in readings:
            groups.setdefault(r["project"], []).append(r["net_lean"])
        for proj, vals in sorted(groups.items(), key=lambda kv: statistics.mean(kv[1])):
            L.append(f"  {axis_cell(statistics.mean(vals))}  {statistics.mean(vals):+.2f}  "
                     f"n={len(vals):<3} {proj}")
    return "\n".join(L)


def render_one(api: str, sid: str) -> str:
    sess = next((s for s in (pd._get(api, "/api/sessions") or {}).get("sessions", [])
                 if s["session_id"] == sid), {"session_id": sid})
    r = reading_for(api, sess)
    L = [f"SINGLE-SESSION READING (the primitive)", "=" * 50,
         f"session {sid[:8]}  ·  {r['project']}  ·  {r['events']} events",
         f"label: {r['label']}", "",
         f"  {axis_cell(r['net_lean'])}  net lean {r['net_lean']:+.2f}", "",
         "signals:"]
    for k, v in r["signals"].items():
        L.append(f"  {k:<18} {v:.2f}")
    return "\n".join(L)


def run_tests() -> int:
    fails = []
    def chk(n, c): (None if c else fails.append(n))
    S = mt.Step
    # a deliberate-ish session: read then edit, low churn
    delib = [S(0, "", "Read", "a"), S(1, "", "Edit", "a"), S(2, "", "Read", "b"), S(3, "", "Edit", "b")]
    # a spontaneous-ish session: re-edits, switches, no reads
    spon = [S(i, "", t, f) for i, (t, f) in enumerate(
        [("Edit", "a"), ("Edit", "b"), ("Edit", "a"), ("Grep", "x"), ("Edit", "c"), ("Edit", "a")])]
    chk("delib>spon", mt.session_reading(delib)["net_lean"] > mt.session_reading(spon)["net_lean"])
    chk("primitive-shape", set(mt.session_reading(delib)) >= {"net_lean", "signals", "n_events"})
    chk("axis-mid", axis_cell(0.0).index("●") == 16)
    chk("axis-left", axis_cell(-1.0).index("●") == 0)
    if fails:
        print("FAIL:", ", ".join(fails)); return 1
    print("ok — session-distribution tests passed"); return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Per-session distribution of the style axis (bottom-up)")
    ap.add_argument("--session", help="show the single-session primitive for one SID")
    ap.add_argument("--days", type=int, default=30)
    ap.add_argument("--sample", type=int, default=60)
    ap.add_argument("--by", choices=["project"], help="group the distribution")
    ap.add_argument("--api", default=pd.DEFAULT_API)
    ap.add_argument("--test", action="store_true")
    args = ap.parse_args()

    if args.test:
        return run_tests()
    if args.session:
        print(render_one(args.api, args.session)); return 0
    print(render(distribution(args.api, args.days, args.sample), args.by))
    return 0


if __name__ == "__main__":
    sys.exit(main())
