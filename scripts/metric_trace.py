"""Procedural epistemology for the archetype metrics — the HOW, à la SICP.

> "Computer science is a procedural epistemology — the study of the structure of
>  knowledge from an imperative point of view." — Abelson & Sussman, SICP, preface

Each archetype signal is not a fact we assert. It is the **value returned by a
procedure applied to your events**. This module is the single home of those
procedures. Every signal is computed here, over a normalized list of `Step`s, and
every result traces to the exact root events it counted:

    metric value  ←  procedure  ←  tool_call CloudEvents (id, seq)  ←  raw JSONL

The same procedures drive both the per-session *trace* (this file's CLI, which
shows the derivation against root event ids) and the windowed *report*
(`archetype_report.py`, which pools the same numerators/denominators). One set of
procedures, two callers, no drift — that is the point.

    python3 scripts/metric_trace.py <SESSION_ID>     # full derivation, traced to root
    python3 scripts/metric_trace.py --self           # trace the current/most-recent session
    python3 scripts/metric_trace.py --test           # procedure self-tests

A `Step` is the normalized unit a procedure consumes:
    {seq, id, tool, target, ts, is_file, is_edit, is_bash}
`id`/`seq` point at the root `tool_call` record; `target` is the file_path (file
tools) or the command (Bash). Steps are built from records (with ids, for tracing)
or from the tool-journey projection (without ids, for the fast windowed report) —
the procedures don't care which, so the counting logic can never diverge.
"""

import argparse
import difflib
import json
import math
import sys
from dataclasses import dataclass, field

import profile_dimensions as pd  # API helper (_get) only

MAX_DIFF_LINES = 120
MAX_LINE = 220
MAX_OUTPUT = 4000

# ── Predicates (the canonical home; archetype_report imports these) ───────────
FILE_TOOLS = {"Read", "Edit", "Write", "NotebookEdit", "Grep", "Glob"}
EDIT_TOOLS = {"Edit", "Write", "NotebookEdit"}
SEARCH_TOOLS = {"Grep", "Glob"}
TEST_RUN_HINTS = ("cargo test", "npm test", "npm run test", "pytest", "vitest",
                  "playwright test", "go test", "just test", "cargo clippy")


def _safe_div(n, d):
    return (n / d) if d else 0.0


# ── Research backing, made first-class (the warrant for every variable) ───────
# The FRAME (deliberate↔spontaneous) is research-backed; the per-signal MAPPING
# to a pole is analogical at best, several are metaphorical, a few are broken.
# theory = registry id (docs/research/developer-archetypes/theory-registry.json).
# strength ∈ validated | analogical | metaphorical | none.
SIGNAL_BACKING = {
    "read_before_edit": dict(pole="deliberate", theory="dietrich-2004", strength="analogical",
        warrant="examine-then-act ≈ controlled, top-down (deliberate) processing",
        flaw="harness tracks file state: Write-then-Edit of an authored file undercounts"),
    "test_cadence": dict(pole="deliberate", theory="dietrich-2004", strength="metaphorical",
        warrant="interleaved verification ≈ analytic loop — BUT testing is a delivery discipline "
                "(dora-2018), not clearly a processing mode; arguably on the wrong axis",
        flaw="hint-list misses `python x.py --test`; false-positives on text containing 'cargo test'"),
    "plan_density": dict(pole="deliberate", theory="dietrich-2004", strength="analogical", scope="window",
        warrant="goal-first planning ≈ deliberate, goal-guided processing",
        flaw="NON-FUNCTIONAL: /plans returns 0; and it is WINDOW-scope (plans ÷ sessions), "
             "so it is undefined per-session — including it in the per-session deliberate "
             "mean (÷3) is the structural tilt bug"),
    "search_fanout": dict(pole="spontaneous", theory="boden-1990", strength="analogical",
        warrant="broad search ≈ exploratory / associative (defocused attention)",
        flaw="search via Bash rg/grep is uncounted → reads ≈0"),
    "context_switch": dict(pole="spontaneous", theory="dietrich-2004", strength="analogical",
        warrant="file-jumping ≈ defocused, associative attention",
        flaw="contested: could be deliberate multi-file coordination"),
    "churn": dict(pole="spontaneous", theory=None, strength="metaphorical",
        warrant="re-editing ≈ trial-and-error (closer to BVSR/Campbell-Simonton than Dietrich)",
        flaw="iterative refinement is often DELIBERATE — the weakest mapping"),
    "seq_entropy": dict(pole="spontaneous", theory=None, strength="metaphorical",
        warrant="varied tool sequences ≈ less-structured/associative (information-theoretic heuristic)",
        flaw="no cited construct; 'structured chains = deliberate' is an untested assumption"),
}

# Stage-level backing — the structure, which is the strongest part of the design.
PIPELINE_BACKING = {
    "process_locus":  dict(theory="rhodes-1961", strength="validated",
        note="reading the PROCESS strand from tool_call events is a legitimate Rhodes locus"),
    "the_axis":       dict(theory="dietrich-2004", strength="validated-frame",
        note="deliberate↔spontaneous is a real construct; our operationalization of it is analogical"),
    "distribution":   dict(theory="dietrich-2004 + space-2021", strength="validated-frame",
        note="mode is task/episode-dependent (Dietrich), not a single metric (SPACE) → show the spread"),
    "constants":      dict(theory=None, strength="none",
        note="0.4, 0.2, equal weights, ±0.08/±0.25 thresholds, 3-vs-4 split: hand-picked, unvalidated; "
             "the 3-vs-4 split + always-zero plan_density tilts the axis toward spontaneous"),
}


# ── Step: the normalized unit, with a pointer to its root event ───────────────
@dataclass
class Step:
    seq: int          # root tool_call seq (ordering within session)
    id: str           # root CloudEvent id ("" if from the id-less journey projection)
    tool: str
    target: str       # file_path for file tools; command for Bash; "" otherwise
    ts: str = ""

    @property
    def is_file(self): return self.tool in FILE_TOOLS and bool(self.target)
    @property
    def is_edit(self): return self.tool in EDIT_TOOLS and bool(self.target)
    @property
    def is_bash(self): return self.tool == "Bash"


def _extract(payload: dict) -> tuple[str, str]:
    """(tool, target) from a tool_call payload. target = file_path or command."""
    name = payload.get("name") or "unknown"
    inp = payload.get("input") or {}
    if name == "Bash":
        return name, (inp.get("command") or "")
    target = inp.get("file_path") or inp.get("notebook_path") or inp.get("path") or ""
    return name, target


def steps_from_records(records: list) -> list[Step]:
    """Build Steps from raw records — carries the root event id+seq."""
    steps = []
    for r in records:
        if r.get("record_type") != "tool_call":
            continue
        tool, target = _extract(r.get("payload") or {})
        steps.append(Step(seq=r.get("seq", 0), id=r.get("id", ""), tool=tool,
                          target=target, ts=r.get("timestamp", "")))
    steps.sort(key=lambda s: s.seq)
    return steps


def steps_from_journey(journey: list) -> list[Step]:
    """Build Steps from the tool-journey projection — no id (windowed report path)."""
    out = []
    for i, j in enumerate(journey):
        out.append(Step(seq=i, id="", tool=j.get("tool") or "",
                        target=j.get("file") or "", ts=j.get("timestamp", "")))
    return out


# ── Derivation: a procedure's result + its full provenance ────────────────────
@dataclass
class Derivation:
    signal: str
    pole: str                 # "deliberate" | "spontaneous"
    procedure: str            # the rule, in words
    numerator: float
    denominator: float
    contributors: list = field(default_factory=list)  # the root events that counted
    lossy: str = ""           # known measurement gap, named honestly

    @property
    def value(self):
        return round(_safe_div(self.numerator, self.denominator), 3)


# ── The procedures (pure: Steps in, Derivation out) ───────────────────────────
def d_read_before_edit(steps: list[Step]) -> Derivation:
    """count edits whose file was Read earlier in the session ÷ total edits."""
    read_set, contributors, num = set(), [], 0
    for s in steps:
        if s.tool == "Read" and s.target:
            read_set.add(s.target)
        elif s.is_edit:
            hit = s.target in read_set
            num += hit
            contributors.append({"seq": s.seq, "id": s.id, "tool": s.tool,
                                 "target": s.target, "read_first": hit})
    return Derivation(
        "read_before_edit", "deliberate",
        "edits whose file was Read earlier ÷ total edits",
        num, len([c for c in contributors]), contributors,
        lossy="the harness tracks file state, so a Write-then-Edit on a file you "
              "just authored counts as NOT read-first even though you hold full context.")


def d_context_switch(steps: list[Step]) -> Derivation:
    """consecutive file-operations whose file changed ÷ file-operations."""
    last, switches, file_steps, contributors = None, 0, 0, []
    for s in steps:
        if not s.is_file:
            continue
        file_steps += 1
        if last is not None and s.target != last:
            switches += 1
            contributors.append({"seq": s.seq, "id": s.id, "from": last, "to": s.target})
        last = s.target
    return Derivation(
        "context_switch", "spontaneous",
        "file-operations whose file differs from the previous file-operation ÷ file-operations",
        switches, file_steps, contributors)


def d_churn(steps: list[Step]) -> Derivation:
    """edits to a file already edited earlier ÷ total edits (re-edit churn)."""
    edited, reedits, edits, contributors = {}, 0, 0, []
    for s in steps:
        if not s.is_edit:
            continue
        edits += 1
        if s.target in edited:
            reedits += 1
            contributors.append({"seq": s.seq, "id": s.id, "target": s.target,
                                 "prior_seq": edited[s.target]})
        edited[s.target] = s.seq
    return Derivation(
        "churn", "spontaneous",
        "edits to a file already edited earlier in the session ÷ total edits",
        reedits, edits, contributors,
        lossy="iterative refinement and trial-and-error are indistinguishable here; "
              "re-editing a file can be deliberate, not only associative.")


def d_test_cadence(steps: list[Step]) -> Derivation:
    """Bash commands that run a test suite ÷ total edits."""
    runs, edits, contributors = 0, 0, []
    for s in steps:
        if s.is_edit:
            edits += 1
        elif s.is_bash:
            cmd = s.target.lower()
            if any(h in cmd for h in TEST_RUN_HINTS):
                runs += 1
                contributors.append({"seq": s.seq, "id": s.id, "cmd": s.target[:80]})
    return Derivation(
        "test_cadence", "deliberate",
        "Bash commands matching a test-runner hint list ÷ total edits",
        runs, edits, contributors,
        lossy=f"the hint list is fixed {TEST_RUN_HINTS}; it MISSES ad-hoc runs like "
              "`python3 script.py --test`, which this project uses constantly.")


def d_seq_entropy(steps: list[Step]) -> Derivation:
    """normalized Shannon entropy of tool-transition bigrams (0..1)."""
    tools = [s.tool for s in steps if s.tool]
    if len(tools) < 3:
        return Derivation("seq_entropy", "spontaneous",
                          "normalized entropy of tool-transition bigrams", 0, 1, [])
    from collections import Counter
    bigrams = Counter(zip(tools, tools[1:]))
    total = sum(bigrams.values())
    h = -sum((c / total) * math.log(c / total) for c in bigrams.values())
    norm = h / math.log(len(bigrams)) if len(bigrams) > 1 else 0.0
    top = [{"bigram": f"{a}→{b}", "n": n} for (a, b), n in bigrams.most_common(6)]
    return Derivation(
        "seq_entropy", "spontaneous",
        "normalized Shannon entropy of tool-transition bigrams (high = varied/jumpy)",
        round(min(1.0, max(0.0, norm)), 3), 1, top,
        lossy="in the windowed report this is averaged across sessions (not pooled), "
              "because entropy is not an additive ratio.")


def d_search_fanout(steps: list[Step]) -> Derivation:
    """Grep+Glob tool-calls ÷ total tool-calls."""
    search = sum(1 for s in steps if s.tool in SEARCH_TOOLS)
    total = sum(1 for s in steps if s.tool)
    contributors = [{"seq": s.seq, "id": s.id, "tool": s.tool} for s in steps if s.tool in SEARCH_TOOLS]
    return Derivation(
        "search_fanout", "spontaneous",
        "Grep+Glob tool-calls ÷ total tool-calls",
        search, total, contributors,
        lossy="search done via `rg`/`grep` inside Bash is NOT counted (it is a Bash "
              "call, not a Grep/Glob tool) — so this understates exploration.")


PROCEDURES = [d_read_before_edit, d_context_switch, d_churn,
              d_test_cadence, d_seq_entropy, d_search_fanout]


def derive_all(steps: list[Step]) -> dict[str, Derivation]:
    return {p(steps).signal: p(steps) for p in PROCEDURES}


# ── Net lean: the ONE combination, shared by trace + windowed report ──────────
def combine_lean(sig: dict) -> dict:
    """Combine the 7 signal VALUES into a deliberate↔spontaneous reading. This is
    the single source of truth — both metric_trace (one session) and
    archetype_report (a window, pooled) call it, so they can never diverge."""
    deliberate = _mean([sig.get("read_before_edit", 0.0),
                        min(1.0, sig.get("test_cadence", 0.0) / 0.4),
                        min(1.0, sig.get("plan_density", 0.0) / 1.0)])
    spontaneous = _mean([min(1.0, sig.get("search_fanout", 0.0) / 0.2),
                         sig.get("context_switch", 0.0),
                         sig.get("churn", 0.0),
                         sig.get("seq_entropy", 0.0)])
    return {"deliberate_index": round(deliberate, 3),
            "spontaneous_index": round(spontaneous, 3),
            "net_lean": round(deliberate - spontaneous, 3)}


def net_lean(derivs: dict[str, Derivation]) -> dict:
    """Session-scope lean from a derivation set (plan_density is window-only → 0)."""
    return combine_lean({name: d.value for name, d in derivs.items()})


def session_reading(steps: list[Step]) -> dict:
    """THE PRIMITIVE: one session's deliberate↔spontaneous reading from its Steps.

    This is the atom the whole archetype is built from. Aggregation is just
    composing many of these — never a separate, pooled computation. Pure: Steps
    in, reading out."""
    derivs = derive_all(steps)
    return {**net_lean(derivs), "n_events": len(steps),
            "signals": {name: d.value for name, d in derivs.items()}}


def _mean(xs):
    xs = [x for x in xs if x is not None]
    return sum(xs) / len(xs) if xs else 0.0


# ── Trace one session against its root events ─────────────────────────────────
def trace(api: str, sid: str) -> dict:
    steps = steps_from_records(records_for(api, sid))
    derivs = derive_all(steps)
    return {
        "session_id": sid,
        "tool_call_events": len(steps),
        "derivations": derivs,
        "lean": net_lean(derivs),
        "first_seq": steps[0].seq if steps else None,
        "last_seq": steps[-1].seq if steps else None,
    }


def records_for(api: str, sid: str) -> list:
    recs = pd._get(api, f"/api/sessions/{sid}/records?limit=5000") or []
    return recs.get("records", []) if isinstance(recs, dict) else recs


def _diff_lines(old: str, new: str) -> tuple[list, bool, dict]:
    """A unified diff old→new as classified lines, plus +/- counts."""
    diff = difflib.unified_diff(old.splitlines(), new.splitlines(), lineterm="", n=2)
    out, added, removed = [], 0, 0
    for ln in diff:
        if ln.startswith("---") or ln.startswith("+++"):
            continue
        k = ("hunk" if ln.startswith("@@") else "add" if ln.startswith("+")
             else "del" if ln.startswith("-") else "ctx")
        added += k == "add"; removed += k == "del"
        out.append({"k": k, "text": ln[:MAX_LINE]})
    truncated = len(out) > MAX_DIFF_LINES
    return out[:MAX_DIFF_LINES], truncated, {"added": added, "removed": removed}


def event_detail(records: list, seq: int) -> dict:
    """The exact action a single tool_call event performed — its literal change.

    Edit/NotebookEdit → unified diff (old_string → new_string).
    Write             → the authored content (as additions).
    Bash/Read/other   → command/target + paired tool_result output.
    Everything traces back to this one CloudEvent; payloads are truncated for
    sanity (local data, the user's own — truncation is for size, not secrecy).
    """
    call = next((r for r in records
                 if r.get("record_type") == "tool_call" and r.get("seq") == seq), None)
    if not call:
        return {"error": f"no tool_call at seq {seq}"}
    p = call.get("payload") or {}
    name = p.get("name") or "unknown"
    inp = p.get("input") or {}
    call_id = p.get("call_id")
    result = next((r for r in records if r.get("record_type") == "tool_result"
                   and (r.get("payload") or {}).get("call_id") == call_id), None)
    out = ((result or {}).get("payload") or {}).get("output")
    out = (out if isinstance(out, str) else json.dumps(out)) if out is not None else ""
    is_error = bool(((result or {}).get("payload") or {}).get("is_error"))

    base = {"seq": seq, "id": call.get("id", ""), "tool": name,
            "target": _repo_rel(inp.get("file_path") or inp.get("command") or ""),
            "is_error": is_error}

    if name in ("Edit", "NotebookEdit"):
        lines, trunc, stats = _diff_lines(inp.get("old_string", ""), inp.get("new_string", ""))
        return {**base, "kind": "diff", "diff": lines, "truncated": trunc, "stats": stats,
                "replace_all": bool(inp.get("replace_all"))}
    if name == "Write":
        content = inp.get("content", "")
        body = content.splitlines()
        lines = [{"k": "add", "text": l[:MAX_LINE]} for l in body[:MAX_DIFF_LINES]]
        return {**base, "kind": "write", "diff": lines, "truncated": len(body) > MAX_DIFF_LINES,
                "stats": {"added": len(body), "removed": 0}}
    if name == "Bash":
        return {**base, "kind": "bash", "command": inp.get("command", ""),
                "output": out[:MAX_OUTPUT], "output_truncated": len(out) > MAX_OUTPUT}
    return {**base, "kind": "other", "input": json.dumps(inp)[:MAX_OUTPUT],
            "output": out[:MAX_OUTPUT]}


def trace_json(api: str, sid: str) -> dict:
    """JSON-serializable trace for the visual — Derivations flattened to dicts."""
    tr = trace(api, sid)
    return {
        "session_id": tr["session_id"],
        "tool_call_events": tr["tool_call_events"],
        "first_seq": tr["first_seq"], "last_seq": tr["last_seq"],
        "lean": tr["lean"],
        "derivations": [{
            "signal": d.signal, "pole": d.pole, "procedure": d.procedure,
            "numerator": d.numerator, "denominator": d.denominator, "value": d.value,
            "lossy": d.lossy,
            "contributors": [{**c, "rel": _repo_rel(c.get("target") or c.get("to") or "")}
                             for c in d.contributors],
        } for d in tr["derivations"].values()],
    }


# ── Render (the procedural story, in text) ────────────────────────────────────
def _repo_rel(t: str) -> str:
    return t.replace("/Users/maxglassie/projects/OpenStory/", "") if t else t


def render_text(tr: dict) -> str:
    L = []
    L.append("PROCEDURAL EPISTEMOLOGY — how this session's archetype is computed")
    L.append("=" * 68)
    L.append(f"session {tr['session_id']}")
    L.append(f"root events: {tr['tool_call_events']} tool_call CloudEvents "
             f"(seq {tr['first_seq']}–{tr['last_seq']}). Each metric below counts a "
             f"subset of these; every counted event is named by its id.\n")
    for d in tr["derivations"].values():
        L.append(f"── {d.signal}   [{d.pole} signal]")
        L.append(f"   procedure : {d.procedure}")
        L.append(f"   value     : {d.numerator:g} / {d.denominator:g} = {d.value}")
        shown = d.contributors[:8]
        if d.signal == "read_before_edit":
            for c in shown:
                mark = "↑ read-first" if c["read_first"] else "✗ not-read  "
                L.append(f"     seq {c['seq']:<4} {mark}  {c['tool']:5} {_repo_rel(c['target'])}  ({c['id'][:8]})")
        elif d.signal == "context_switch":
            for c in shown:
                L.append(f"     seq {c['seq']:<4} {_repo_rel(c['from'])} → {_repo_rel(c['to'])}  ({c['id'][:8]})")
        elif d.signal == "churn":
            for c in shown:
                L.append(f"     seq {c['seq']:<4} re-edit {_repo_rel(c['target'])} (first at seq {c['prior_seq']})  ({c['id'][:8]})")
        elif d.signal == "test_cadence":
            for c in shown:
                L.append(f"     seq {c['seq']:<4} {c['cmd']}  ({c['id'][:8]})")
            if not shown:
                L.append("     (no matches)")
        elif d.signal == "seq_entropy":
            for c in shown:
                L.append(f"     {c['bigram']:<28} ×{c['n']}")
        elif d.signal == "search_fanout":
            for c in shown:
                L.append(f"     seq {c['seq']:<4} {c['tool']}  ({c['id'][:8]})")
            if not shown:
                L.append("     (no Grep/Glob tool-calls)")
        if len(d.contributors) > 8:
            L.append(f"     … +{len(d.contributors)-8} more")
        if d.lossy:
            L.append(f"   ⚠ lossy   : {d.lossy}")
        L.append("")
    ln = tr["lean"]
    L.append(f"NET LEAN = deliberate {ln['deliberate_index']} − spontaneous "
             f"{ln['spontaneous_index']} = {ln['net_lean']:+.3f}")
    L.append("(value ← procedure ← the tool_call events above ← the raw JSONL transcript)")
    return "\n".join(L)


# ── Self-tests (procedures are pure — assert exact values) ────────────────────
def run_tests() -> int:
    fails = []
    def chk(n, c): (None if c else fails.append(n))

    def S(seq, tool, target): return Step(seq, f"id{seq}", tool, target)
    # Read a, Edit a (read-first), Write b (not read), Edit b (re-edit + not read)
    steps = [S(0, "Read", "a"), S(1, "Edit", "a"), S(2, "Write", "b"), S(3, "Edit", "b")]
    rbe = d_read_before_edit(steps)
    chk("rbe-num", rbe.numerator == 1)          # only Edit a was read-first
    chk("rbe-den", rbe.denominator == 3)        # Edit a, Write b, Edit b
    chk("rbe-val", rbe.value == round(1/3, 3))
    churn = d_churn(steps)
    chk("churn", churn.numerator == 1 and churn.denominator == 3)  # Edit b re-edits b
    sw = d_context_switch(steps)
    # files a,a,b,b → switches: a==a no, a!=b yes, b==b no → 1 switch / 4 file-steps
    chk("switch", sw.numerator == 1 and sw.denominator == 4)
    tc = d_test_cadence([S(0, "Bash", "cargo test -p x"), S(1, "Edit", "a")])
    chk("test-cadence", tc.numerator == 1 and tc.denominator == 1)
    tc2 = d_test_cadence([S(0, "Bash", "python3 x.py --test"), S(1, "Edit", "a")])
    chk("test-cadence-miss", tc2.numerator == 0)   # documents the known gap
    sf = d_search_fanout([S(0, "Grep", "x"), S(1, "Bash", "rg foo"), S(2, "Read", "a")])
    chk("search-fanout", sf.numerator == 1 and sf.denominator == 3)  # bash rg not counted
    ent = d_seq_entropy([S(i, t, "") for i, t in enumerate(["Read","Read","Read","Read"])])
    chk("entropy-flat", ent.value == 0.0)

    if fails:
        print("FAIL:", ", ".join(fails)); return 1
    print("ok — metric-procedure self-tests passed"); return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Trace each archetype metric to its root events (SICP-style)")
    ap.add_argument("session_id", nargs="?", help="session UUID to trace")
    ap.add_argument("--self", action="store_true", help="trace the most-recently-active session")
    ap.add_argument("--api", default=pd.DEFAULT_API)
    ap.add_argument("--format", choices=["text", "json"], default="text")
    ap.add_argument("--test", action="store_true")
    args = ap.parse_args()

    if args.test:
        return run_tests()

    sid = args.session_id
    if args.self or not sid:
        sessions = (pd._get(args.api, "/api/sessions") or {}).get("sessions", [])
        if not sessions:
            print("no sessions found"); return 1
        sid = max(sessions, key=lambda s: s.get("last_event", ""))["session_id"]

    tr = trace(args.api, sid)
    if args.format == "json":
        out = {**tr, "derivations": {k: {**vars(v), "value": v.value} for k, v in tr["derivations"].items()}}
        print(json.dumps(out, indent=2, default=str))
    else:
        print(render_text(tr))
    return 0


if __name__ == "__main__":
    sys.exit(main())
