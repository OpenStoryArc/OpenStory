"""First-draft citation-traceable archetype report from OpenStory data.

Computes the one creative-style dimension OpenStory can honestly observe —
Dietrich's (2004) deliberate ↔ spontaneous processing mode — from real session
events, attaches its backing from the verified theory registry, and renders a
report that states its warrant strength AND what it refuses to claim.

This is the synthesis of everything in docs/research/developer-archetypes/:
the rubric is a Toulmin argument (claim ← grounds ← warrant ← backing ← qualifier
← rebuttal), every backing traces to a primary source, and the cognitive↔emotional
axis / production-incident DORA metrics / the refuted "Conductor" are named as
explicit non-claims. Local-only; nothing leaves the machine.

    python3 scripts/archetype_report.py                 # last 30 days, text report
    python3 scripts/archetype_report.py --days 90 --sample 60
    python3 scripts/archetype_report.py --format md --out /tmp/report.md
    python3 scripts/archetype_report.py --test          # pure-scoring self-tests
"""

import argparse
import json
import math
import sys
from collections import Counter
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timedelta, timezone
from pathlib import Path

import profile_dimensions as pd  # reuse _get, path helpers
import metric_trace as mt        # the canonical per-signal procedures (no drift)

REGISTRY = Path(__file__).resolve().parent.parent / "docs/research/developer-archetypes/theory-registry.json"


def _mean(xs):
    xs = [x for x in xs if x is not None]
    return sum(xs) / len(xs) if xs else 0.0


def lean_label(net: float) -> str:
    if net >= 0.25: return "strongly deliberate"
    if net >= 0.08: return "deliberate-leaning"
    if net > -0.08: return "balanced / ambidextrous"
    if net > -0.25: return "spontaneous-leaning"
    return "strongly spontaneous"


# ── Data collection (side effects at the edge) ───────────────────────────────
def collect_signals(api: str, days: int, sample: int) -> dict:
    cutoff = datetime.now(timezone.utc) - timedelta(days=days)
    sessions = (pd._get(api, "/api/sessions") or {}).get("sessions", [])
    sessions = [s for s in sessions if pd._within(s.get("last_event", ""), cutoff)]
    if not sessions:
        return {"empty": True, "days": days}

    top = sorted(sessions, key=lambda s: s.get("event_count", 0), reverse=True)[:sample]
    ids = [s["session_id"] for s in top]

    def fetch(sid):
        return (
            sid,
            pd._get(api, f"/api/sessions/{sid}/tool-journey") or [],
            pd._get(api, f"/api/sessions/{sid}/plans") or {},
        )

    # The windowed report POOLS the exact per-session procedures from metric_trace.
    # ratio signals: sum numerators / sum denominators. entropy: mean (not additive).
    RATIOS = ["read_before_edit", "context_switch", "churn", "test_cadence", "search_fanout"]
    acc = {name: [0.0, 0.0] for name in RATIOS}
    entropies, plans_total, flow = [], 0, Counter()

    with ThreadPoolExecutor(max_workers=8) as pool:
        for sid, journey, plans in pool.map(fetch, ids):
            steps = mt.steps_from_journey(journey if isinstance(journey, list) else [])
            derivs = mt.derive_all(steps)
            for name in RATIOS:
                acc[name][0] += derivs[name].numerator
                acc[name][1] += derivs[name].denominator
            entropies.append(derivs["seq_entropy"].value)
            for s in steps:                      # delivery-flow from the same steps
                if s.is_bash:
                    _delivery_signatures(s.target.lower(), flow)
            plist = plans.get("plans", plans) if isinstance(plans, dict) else plans
            plans_total += len(plist) if isinstance(plist, list) else 0

    sig = {name: (acc[name][0] / acc[name][1] if acc[name][1] else 0.0) for name in RATIOS}
    sig["seq_entropy"] = _mean(entropies)
    sig["plan_density"] = plans_total / max(1, len(ids))
    return {
        "empty": False, "days": days, "session_count": len(sessions),
        "sampled": len(ids), "signals": sig, "flow": dict(flow),
    }


def _delivery_signatures(cmd: str, flow: Counter) -> None:
    """Session-observable DORA-flavored signals only. CFR/MTTR need prod data."""
    if "git commit" in cmd: flow["commit"] += 1
    if "git push" in cmd: flow["push"] += 1
    if "git checkout -b" in cmd or "git switch -c" in cmd: flow["branch"] += 1
    if "git revert" in cmd or "git reset --hard" in cmd: flow["revert"] += 1


# ── Registry + report rendering ──────────────────────────────────────────────
def load_registry() -> dict:
    data = json.loads(REGISTRY.read_text())
    return {r["id"]: r for r in data["records"]}


def cite(reg: dict, tid: str) -> str:
    r = reg.get(tid)
    if not r:
        return f"[missing:{tid}]"
    return f"{r['scholar']} ({r['year']}). {r['primary_work']}. <{r['primary_source']}>"


def build_report(data: dict, reg: dict) -> dict:
    sig = data["signals"]
    ds = mt.combine_lean(sig)
    label = lean_label(ds["net_lean"])

    style_assessment = {
        "axis": "creative style — deliberate ↔ spontaneous (processing mode)",
        "claim": label,
        "grounds": {**sig, **ds, "sampled_sessions": data["sampled"]},
        "warrant": ("Plan-first, read-before-edit, test-interleaved, low-entropy tool "
                    "chains indicate deliberate (controlled) processing; search fan-out, "
                    "context switching, re-edit churn and high sequence entropy indicate "
                    "spontaneous (associative) processing."),
        "backing": [{"theory_id": "dietrich-2004", "warrant_strength": "analogical"}],
        "qualifier": ("warrant_strength = ANALOGICAL (validated-ELIGIBLE, not yet "
                      "validated): the signal→construct mapping has not been empirically "
                      "checked against an independent measure."),
        "rebuttal": ("Refuted if deliberate-signal sessions do not differ from "
                     "spontaneous-signal sessions on any independent indicator of "
                     "controlled vs. associative work."),
    }
    return {"style": style_assessment, "ds": ds, "flow": data["flow"], "data": data}


# How each signal is normalized inside combine_lean, and which pole it feeds.
# (pole, transform) — must mirror mt.combine_lean exactly.
SIGNAL_POLE = {
    "read_before_edit": ("deliberate", lambda v: v),
    "test_cadence":     ("deliberate", lambda v: min(1.0, v / 0.4)),
    "plan_density":     ("deliberate", lambda v: min(1.0, v / 1.0)),
    "search_fanout":    ("spontaneous", lambda v: min(1.0, v / 0.2)),
    "context_switch":   ("spontaneous", lambda v: v),
    "churn":            ("spontaneous", lambda v: v),
    "seq_entropy":      ("spontaneous", lambda v: v),
}
NICE = {"read_before_edit": "reads before editing", "test_cadence": "runs tests",
        "plan_density": "plans first", "search_fanout": "searches wide",
        "context_switch": "switches files", "churn": "re-edits in place",
        "seq_entropy": "uses varied tool sequences"}


def interpret(sig: dict, ds: dict) -> dict:
    """The relationships, made explicit: what the lean MEANS, and WHICH signals
    drove it. net = mean(deliberate)/1 − mean(spontaneous); 3 deliberate signals,
    4 spontaneous, so a signal's signed push on net is +norm/3 or −norm/4."""
    drivers = []
    for name, (pole, tf) in SIGNAL_POLE.items():
        norm = tf(sig.get(name, 0.0))
        contribution = (norm / 3.0) if pole == "deliberate" else -(norm / 4.0)
        drivers.append({"signal": name, "nice": NICE[name], "pole": pole,
                        "raw": round(sig.get(name, 0.0), 2), "normalized": round(norm, 2),
                        "contribution": round(contribution, 3)})
    drivers.sort(key=lambda d: d["contribution"])  # most spontaneous push first
    lean = ds["net_lean"]
    if lean <= -0.08:
        meaning = ("you tend to work **associatively** — exploring, switching between files, and "
                   "iterating in place, rather than planning then executing in long, ordered chains.")
    elif lean >= 0.08:
        meaning = ("you tend to work **deliberately** — reading and planning before you change "
                   "things, moving in ordered, sequential chains rather than jumping and iterating.")
    else:
        meaning = ("you move fluidly between **deliberate** planning and **associative** exploration "
                   "— neither pole dominates how you work.")
    return {"meaning": meaning, "drivers": drivers}


def resolve(reg: dict, tid: str) -> dict:
    r = reg[tid]
    return {"id": tid, "scholar": r["scholar"], "year": r["year"],
            "primary_work": r["primary_work"], "primary_source": r["primary_source"],
            "measures": r["measures"], "kind": r["kind"]}


def build_payload(api: str, days: int, sample: int) -> dict:
    """Fully-resolved JSON payload — shared by the CLI and the live HTML view, so
    the report never forks. Resolves every backing id to its registry citation."""
    data = collect_signals(api, days, sample)
    if data.get("empty"):
        return {"empty": True, "days": days}
    reg = load_registry()
    rep = build_report(data, reg)
    s = rep["style"]; ds = rep["ds"]; sig = data["signals"]
    interp = interpret(sig, ds)

    caveats = []
    if sig["plan_density"] == 0:
        caveats.append("plan_density = 0: plan-capture is off in this data — a DELIBERATE "
                       "signal reading structurally zero, so deliberate is understated.")
    if sig["search_fanout"] == 0:
        caveats.append("search_fanout = 0: search via rg/grep in Bash isn't counted as "
                       "Grep/Glob — a SPONTANEOUS signal reading zero, so spontaneous is "
                       "understated. The two gaps pull opposite ways; the net lean is soft.")

    provenance = []
    for tid in ["dietrich-2004", "dora-2018", "space-2021", "sawyer-2009"]:
        r = reg[tid]
        note = ("REFUTES the orchestration framing" if tid == "sawyer-2009"
                else "validated-eligible (process locus)" if r["measures"] == "process"
                else "analogical only")
        provenance.append({**resolve(reg, tid), "note": note})

    return {
        "empty": False, "days": days, "session_count": data["session_count"],
        "sampled": data["sampled"],
        "style": {
            "claim": s["claim"], "net_lean": ds["net_lean"],
            "deliberate_index": ds["deliberate_index"],
            "spontaneous_index": ds["spontaneous_index"],
            "signals": sig, "warrant": s["warrant"], "qualifier": s["qualifier"],
            "rebuttal": s["rebuttal"],
            "meaning": interp["meaning"], "drivers": interp["drivers"],
            "backing": [{**b, "citation": resolve(reg, b["theory_id"])} for b in s["backing"]],
        },
        "flow": rep["flow"],
        "flow_backing": [resolve(reg, "dora-2018"), resolve(reg, "space-2021")],
        "refusals": [{"title": t, "why": w} for t, w in REFUSALS],
        "caveats": caveats,
        "provenance": provenance,
    }


REFUSALS = [
    ("Cognitive ↔ emotional axis", "Dietrich's second axis is not observable from "
     "process events — we see what was done, not the affect driving it. The style axis "
     "is a line, not a quadrant. (parked, not faked)"),
    ("\"The Conductor\" / any orchestration archetype", "Refuted: Sawyer & DeZutter "
     "(2009) place the conductor-led orchestra at the predictable pole, the opposite of "
     "creative emergence. No orchestration name may cite that work."),
    ("DORA Elite/High/Medium/Low tier", "Popular-reporting artifact, not in the 2018 "
     "book and not peer-reviewed (the academic study found three clusters)."),
    ("Deployment frequency / change-failure-rate / MTTR", "Need production & incident "
     "data. A session observer sees only commits, pushes, branches, reverts, tests."),
]


def render_md(rep: dict, reg: dict, days: int) -> str:
    d = rep["data"]; s = rep["style"]; ds = rep["ds"]; g = s["grounds"]; flow = rep["flow"]
    L = []
    L.append("# Your OpenStory Developer Profile — first draft\n")
    L.append(f"*Local-only. Computed from {d['session_count']} sessions over the last "
             f"{days} days (deep-read top {d['sampled']}). Every conclusion is traceable "
             f"up to a primary source and down to your events.*\n")

    L.append("## Creative style — deliberate ↔ spontaneous\n")
    L.append("```")
    L.append(_gauge(ds["net_lean"]))
    L.append("```")
    L.append(f"**{s['claim']}**  (net lean {ds['net_lean']:+.2f}; "
             f"deliberate {ds['deliberate_index']:.2f} vs spontaneous {ds['spontaneous_index']:.2f})\n")
    L.append("| signal | value | pole |")
    L.append("|---|--:|---|")
    L.append(f"| read-before-edit | {g['read_before_edit']:.2f} | deliberate |")
    L.append(f"| test cadence (runs/edit) | {g['test_cadence']:.2f} | deliberate |")
    L.append(f"| plan density (plans/session) | {g['plan_density']:.2f} | deliberate |")
    L.append(f"| search fan-out | {g['search_fanout']:.2f} | spontaneous |")
    L.append(f"| context switching | {g['context_switch']:.2f} | spontaneous |")
    L.append(f"| re-edit churn | {g['churn']:.2f} | spontaneous |")
    L.append(f"| tool-sequence entropy | {g['seq_entropy']:.2f} | spontaneous |\n")

    L.append("**Why this reading (the warrant chain):**\n")
    L.append(f"- *Claim* — {s['claim']}")
    L.append(f"- *Warrant* — {s['warrant']}")
    L.append(f"- *Backing* — {cite(reg, 'dietrich-2004')}")
    L.append(f"- *Qualifier* — {s['qualifier']}")
    L.append(f"- *Rebuttal* — {s['rebuttal']}\n")

    L.append("## Delivery flow — session-observable signals only\n")
    if flow:
        L.append("Across the sampled sessions: " +
                 ", ".join(f"**{k}** ×{v}" for k, v in sorted(flow.items(), key=lambda kv: -kv[1])) + ".")
    else:
        L.append("No commit/push/branch/revert signatures in the sampled sessions.")
    L.append(f"\n*Backing — {cite(reg, 'dora-2018')}; framed per {cite(reg, 'space-2021')} "
             "(\"cannot be measured by a single metric\"). This axis is a style, not a score.*\n")

    L.append("## What this report refuses to claim\n")
    L.append("*The honest core: an assessment you can audit is one that names its own limits.*\n")
    for title, why in REFUSALS:
        L.append(f"- **{title}** — {why}")
    L.append("")

    L.append("## Provenance\n")
    L.append("Backing constructs, each verified against a primary source by adversarial "
             "(3-vote) deep research:\n")
    for tid in ["dietrich-2004", "dora-2018", "space-2021", "sawyer-2009"]:
        r = reg[tid]
        tag = "REFUTES the orchestration framing" if tid == "sawyer-2009" else \
              f"warrant_strength when applied to our process signal: {'analogical' if r['measures'] in ('person','product') else 'validated-eligible'}"
        L.append(f"- `{tid}` — {cite(reg, tid)} — measures **{r['measures']}** · {tag}")
    L.append("\n*Full registry: `docs/research/developer-archetypes/theory-registry.json` "
             "(16 verified records). Method: `registry-schema.md`.*")
    return "\n".join(L)


def _gauge(net: float, width: int = 41) -> str:
    mid = width // 2
    pos = int(round(mid + net * mid))
    pos = max(0, min(width - 1, pos))
    bar = ["·"] * width
    bar[mid] = "|"
    bar[pos] = "●"
    return ("spontaneous " + "".join(bar) + " deliberate")


# ── Self-tests ───────────────────────────────────────────────────────────────
def run_tests() -> int:
    fails = []
    def chk(n, c): (None if c else fails.append(n))

    Step = mt.Step
    seq = [Step(i, "", t, "") for i, t in enumerate(["Read", "Read", "Read", "Read"])]
    chk("entropy-flat", mt.d_seq_entropy(seq).value == 0.0)
    varied = [Step(i, "", t, "") for i, t in enumerate(["Read", "Edit", "Bash", "Grep", "Read", "Write"])]
    chk("entropy-varied", mt.d_seq_entropy(varied).value > 0.5)

    deliberate_sig = {"read_before_edit": 0.9, "test_cadence": 0.4, "plan_density": 1.0,
                      "search_fanout": 0.02, "context_switch": 0.2, "churn": 0.1, "seq_entropy": 0.2}
    spontaneous_sig = {"read_before_edit": 0.2, "test_cadence": 0.0, "plan_density": 0.0,
                       "search_fanout": 0.25, "context_switch": 0.8, "churn": 0.6, "seq_entropy": 0.85}
    chk("deliberate-positive", mt.combine_lean(deliberate_sig)["net_lean"] > 0.3)
    chk("spontaneous-negative", mt.combine_lean(spontaneous_sig)["net_lean"] < -0.3)
    chk("label-delib", lean_label(0.4) == "strongly deliberate")
    chk("label-balanced", lean_label(0.0) == "balanced / ambidextrous")

    # registry must load and contain dietrich-2004 with process locus
    reg = load_registry()
    chk("registry-dietrich", reg.get("dietrich-2004", {}).get("measures") == "process")
    chk("registry-sawyer-refutes", "REFUT" in (reg.get("sawyer-2009", {}).get("history_note", "")).upper())

    if fails:
        print("FAIL:", ", ".join(fails)); return 1
    print("ok — archetype-report self-tests passed"); return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="First-draft citation-traceable archetype report")
    ap.add_argument("--days", type=int, default=30)
    ap.add_argument("--sample", type=int, default=50)
    ap.add_argument("--api", default=pd.DEFAULT_API)
    ap.add_argument("--format", choices=["md", "json"], default="md")
    ap.add_argument("--out", help="write the report to this path instead of stdout")
    ap.add_argument("--test", action="store_true")
    args = ap.parse_args()

    if args.test:
        return run_tests()

    data = collect_signals(args.api, args.days, args.sample)
    if data.get("empty"):
        print(f"No sessions in the last {args.days} days. Is OpenStory running on {args.api}?")
        return 1
    reg = load_registry()
    rep = build_report(data, reg)

    out = json.dumps(rep["style"], indent=2) if args.format == "json" else render_md(rep, reg, args.days)
    if args.out:
        Path(args.out).write_text(out)
        print(f"wrote {args.out}")
    else:
        print(out)
    return 0


if __name__ == "__main__":
    sys.exit(main())
