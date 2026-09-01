"""Developer-profile prototype — a sovereign, local "Wrapped" from OpenStory data.

Inspired by YC's Paxel (paxel.ycombinator.com), which scores developers across
five dimensions (steering, execution, engineering, product instinct, planning)
and assigns an archetype. Paxel ships anonymized score payloads to YC servers.
This prototype computes the same shape of profile **entirely locally** from the
OpenStory REST API — nothing leaves the machine. That is the differentiator: the
profile is derived from your own event store, on your terms.

This is a SPEC, not production code (CLAUDE.md §8 "shift prototyping left"). The
job is to validate the rubric against real data before any Rust query is written.
Every score is an absolute heuristic on a tunable reference scale — because the
data is local there is no cross-user corpus to take percentiles against (that
would later flow over your own NATS federation, never a central server).

Signals (all via the dogfooded REST API, never raw file grep — CLAUDE.md):
  GET /api/sessions               per-session events, tokens, branch, agent
  GET /api/insights/efficiency    per-session duration, error_count, tool_count
  GET /api/insights/productivity  hour-of-day event histogram
  GET /api/insights/pulse         per-project rollups (breadth)
  GET /api/sessions/{id}/tools         tool histogram (sampled sessions)
  GET /api/sessions/{id}/tool-journey  per-step (tool, file/command) stream
  GET /api/sessions/{id}/patterns      eval-apply scope depth (decomposition)
  GET /api/sessions/{id}/plans         plan documents (structure-before-action)

KNOWN WEAK SIGNALS (surfaced by running this against real data — fix before any
production port): the `planning` dimension is currently under-powered because
(a) /plans returns 0 plan documents per session in the live store, and
(b) eval-apply `scope_depth` is uniformly 0 across every pattern, so it cannot
differentiate decomposition. Until those are fixed, a low `planning` score
reflects missing instrumentation as much as developer behavior. The honest move
is to fix the upstream signals (plan extraction + scope_depth tracking) rather
than re-weight the rubric to hide the gap.

Usage:
    python3 scripts/profile_dimensions.py                 # last 30 days, text card
    python3 scripts/profile_dimensions.py --days 90
    python3 scripts/profile_dimensions.py --sample 80     # deep-fetch top-80 sessions
    python3 scripts/profile_dimensions.py --format json
    python3 scripts/profile_dimensions.py --test          # run self-tests
"""

import argparse
import json
import math
import sys
import urllib.request
import urllib.error
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field, asdict
from datetime import datetime, timedelta, timezone
from typing import Optional


DEFAULT_API = "http://localhost:3002"

# ── The five dimensions (Paxel's taxonomy) ──────────────────────────────────
DIMENSIONS = ["steering", "execution", "engineering", "product", "planning"]

# Archetype names keyed by the dominant dimension. The secondary dimension adds
# a modifier so two people who both lead on "execution" can still differ.
ARCHETYPE = {
    "steering": "The Conductor",
    "execution": "Velocity Machine",
    "engineering": "Quality Guardian",
    "product": "Product Visionary",
    "planning": "The Architect",
}


# ── Pure scoring helpers ────────────────────────────────────────────────────
def norm(value: float, lo: float, hi: float) -> float:
    """Map value in [lo, hi] onto a 0–10 score, clamped at the ends."""
    if hi == lo:
        return 0.0
    t = (value - lo) / (hi - lo)
    return max(0.0, min(10.0, t * 10.0))


def norm_inverse(value: float, tight: float, loose: float) -> float:
    """Smaller-is-better score: `tight` -> 10, `loose` -> 0."""
    return norm(value, loose, tight)


def blend(*pairs: tuple[float, float]) -> float:
    """Weighted average of (score, weight) pairs. Weights need not sum to 1."""
    total_w = sum(w for _, w in pairs) or 1.0
    return sum(s * w for s, w in pairs) / total_w


# ── Metrics extracted from the API (everything a dimension reads from) ───────
@dataclass
class Metrics:
    session_count: int = 0
    avg_tools_per_session: float = 0.0  # robust to idle gaps (wall-clock is not)
    avg_events_per_session: float = 0.0
    write_intensity: float = 0.0      # (Edit+Write) / tools
    read_intensity: float = 0.0       # Read / tools
    search_intensity: float = 0.0     # (Grep+Glob) / tools
    delegation: float = 0.0           # (Agent+Task) / tools
    collaboration: float = 0.0        # AskUserQuestion / tools
    planning_tools: float = 0.0       # (TodoWrite+Exit/EnterPlanMode) / tools
    error_recovery: float = 0.0       # 1 - clamp(errors per tool / 0.2)
    test_file_fraction: float = 0.0   # touched files that look like tests
    doc_file_fraction: float = 0.0    # touched files that look like docs
    read_before_edit: float = 0.0     # edited files that were Read first
    project_entropy: float = 0.0      # normalized Shannon entropy over projects
    plans_per_session: float = 0.0
    avg_scope_depth: float = 0.0      # eval-apply nesting depth (decomposition)

    # Descriptive (not scored) — for the "signature stats" on the card.
    peak_hour: int = 0
    night_fraction: float = 0.0
    top_project: str = ""
    project_count: int = 0
    agent_mix: dict = field(default_factory=dict)
    total_events: int = 0


def score_dimensions(m: Metrics) -> dict[str, float]:
    """The rubric. Each dimension is a documented blend of normalized signals.

    Reference scales are heuristics, tunable in one place. They encode "what a
    10 looks like" in the absence of a cross-user corpus.
    """
    focus = norm_inverse(m.avg_events_per_session, tight=40, loose=600)

    steering = blend(
        (norm(m.session_count, 3, 60), 0.40),       # more sessions = more active direction
        (norm(m.collaboration, 0.0, 0.05), 0.35),   # checking in with the human
        (focus, 0.25),                              # tight loops, not runaway sessions
    )
    execution = blend(
        (norm(m.avg_tools_per_session, 5, 80), 0.45),  # intensity per session, not per wall-clock hour
        (norm(m.write_intensity, 0.05, 0.5), 0.35),
        (norm(m.delegation, 0.0, 0.15), 0.20),         # fanning out to subagents = scale
    )
    engineering = blend(
        (norm(m.read_intensity, 0.05, 0.45), 0.30),
        (norm(m.read_before_edit, 0.3, 0.95), 0.25),
        (norm(m.test_file_fraction, 0.0, 0.30), 0.25),
        (m.error_recovery * 10, 0.20),  # recovery is already 0–1
    )
    product = blend(
        (norm(m.project_entropy, 0.0, 1.0), 0.45),  # breadth across projects
        (norm(m.doc_file_fraction, 0.0, 0.30), 0.35),
        (norm(m.plans_per_session, 0.0, 2.0), 0.20),
    )
    planning = blend(
        (norm(m.plans_per_session, 0.0, 2.0), 0.40),
        (norm(m.avg_scope_depth, 0.0, 3.0), 0.30),
        (norm(m.planning_tools, 0.0, 0.06), 0.30),
    )
    return {
        "steering": round(steering, 1),
        "execution": round(execution, 1),
        "engineering": round(engineering, 1),
        "product": round(product, 1),
        "planning": round(planning, 1),
    }


def classify(scores: dict[str, float]) -> tuple[str, str]:
    """Return (archetype_name, one-line rationale) from the dimension scores."""
    ranked = sorted(scores.items(), key=lambda kv: kv[1], reverse=True)
    top, top_v = ranked[0]
    second, second_v = ranked[1]
    name = ARCHETYPE[top]
    # If two dimensions are within 0.5, name the blend.
    if top_v - second_v <= 0.5:
        name = f"{ARCHETYPE[top]} · {second} hybrid"
    rationale = f"led by {top} ({top_v}), backed by {second} ({second_v})"
    return name, rationale


def growth_edge(scores: dict[str, float]) -> tuple[str, float]:
    """The lowest dimension — the honest growth opportunity."""
    return min(scores.items(), key=lambda kv: kv[1])


# ── API fetch layer (side effects live here, at the edge) ───────────────────
def _get(api: str, path: str) -> Optional[object]:
    url = f"{api}{path}"
    try:
        with urllib.request.urlopen(url, timeout=20) as r:
            return json.loads(r.read())
    except (urllib.error.URLError, json.JSONDecodeError, TimeoutError) as e:
        print(f"  ! fetch failed {path}: {e}", file=sys.stderr)
        return None


def _within(ts: str, cutoff: datetime) -> bool:
    try:
        return datetime.fromisoformat(ts.replace("Z", "+00:00")) >= cutoff
    except (ValueError, AttributeError):
        return False


def _is_test_path(f: str) -> bool:
    f = (f or "").lower()
    return any(k in f for k in ("/test", "test_", "_test.", ".test.", "spec", "conformance", "/tests/"))


def _is_doc_path(f: str) -> bool:
    f = (f or "").lower()
    return f.endswith((".md", ".mdx", ".rst")) or "/docs/" in f


def collect_metrics(api: str, days: int, sample: int) -> Metrics:
    """Pull the cheap window-wide aggregates, then deep-fetch the top-N sessions."""
    cutoff = datetime.now(timezone.utc) - timedelta(days=days)

    sessions = (_get(api, "/api/sessions") or {}).get("sessions", [])
    sessions = [s for s in sessions if _within(s.get("last_event", ""), cutoff)]
    productivity = _get(api, "/api/insights/productivity") or []
    efficiency = _get(api, "/api/insights/efficiency") or []
    pulse = _get(api, "/api/insights/pulse") or []

    m = Metrics()
    m.session_count = len(sessions)
    if not sessions:
        return m

    m.total_events = sum(s.get("event_count", 0) for s in sessions)
    m.avg_events_per_session = m.total_events / max(1, len(sessions))

    # Agent / model mix.
    for s in sessions:
        a = s.get("origin_agent", "unknown")
        m.agent_mix[a] = m.agent_mix.get(a, 0) + 1

    # Project breadth → normalized Shannon entropy (1.0 = perfectly spread).
    proj_counts: dict[str, int] = {}
    for s in sessions:
        p = s.get("project_name") or s.get("project_id") or "?"
        proj_counts[p] = proj_counts.get(p, 0) + 1
    m.project_count = len(proj_counts)
    if m.project_count > 1:
        total = sum(proj_counts.values())
        h = -sum((c / total) * math.log(c / total) for c in proj_counts.values())
        m.project_entropy = h / math.log(m.project_count)
    m.top_project = max(proj_counts, key=proj_counts.get) if proj_counts else ""

    # Time-of-day signature.
    if productivity:
        m.peak_hour = max(productivity, key=lambda h: h["event_count"])["hour"]
        night = sum(h["event_count"] for h in productivity if h["hour"] < 6 or h["hour"] >= 22)
        tot = sum(h["event_count"] for h in productivity) or 1
        m.night_fraction = night / tot

    # Efficiency: throughput + error recovery, window-filtered by session id.
    win_ids = {s["session_id"] for s in sessions}
    eff = [e for e in efficiency if e.get("session_id") in win_ids]
    if eff:
        tool_sum = sum(e.get("tool_count", 0) for e in eff)
        err_sum = sum(e.get("error_count", 0) for e in eff)
        m.avg_tools_per_session = tool_sum / max(1, len(eff))
        errs_per_tool = err_sum / max(1, tool_sum)
        m.error_recovery = max(0.0, 1.0 - min(1.0, errs_per_tool / 0.2))

    # ── Deep sample: top-N sessions by event_count ──
    top = sorted(sessions, key=lambda s: s.get("event_count", 0), reverse=True)[:sample]
    ids = [s["session_id"] for s in top]

    tool_totals: dict[str, int] = {}
    plans_total = 0
    scope_depths: list[int] = []

    def deep(sid: str):
        return (
            sid,
            _get(api, f"/api/sessions/{sid}/tools") or {},
            _get(api, f"/api/sessions/{sid}/patterns") or {},
            _get(api, f"/api/sessions/{sid}/plans") or {},
            _get(api, f"/api/sessions/{sid}/tool-journey") or [],
        )

    journeys: dict[str, list] = {}
    with ThreadPoolExecutor(max_workers=8) as pool:
        for sid, tools, patterns, plans, journey in pool.map(deep, ids):
            for t, c in (tools.items() if isinstance(tools, dict) else []):
                tool_totals[t] = tool_totals.get(t, 0) + c

            pats = patterns.get("patterns", []) if isinstance(patterns, dict) else []
            for p in pats:
                d = (p.get("metadata") or {}).get("scope_depth")
                if isinstance(d, int):
                    scope_depths.append(d)

            plist = plans.get("plans", plans) if isinstance(plans, dict) else plans
            plans_total += len(plist) if isinstance(plist, list) else 0

            journeys[sid] = journey if isinstance(journey, list) else []

    # Tool-mix fractions (from aggregated histogram).
    tot_tools = sum(tool_totals.values()) or 1
    def frac(*names):
        return sum(tool_totals.get(n, 0) for n in names) / tot_tools
    m.write_intensity = frac("Edit", "Write", "NotebookEdit")
    m.read_intensity = frac("Read")
    m.search_intensity = frac("Grep", "Glob")
    m.delegation = frac("Agent", "Task")
    m.collaboration = frac("AskUserQuestion")
    m.planning_tools = frac("TodoWrite", "ExitPlanMode", "EnterPlanMode")

    m.plans_per_session = plans_total / max(1, len(ids))
    m.avg_scope_depth = (sum(scope_depths) / len(scope_depths)) if scope_depths else 0.0

    # File-path signals from the tool journeys we already fetched.
    _enrich_file_signals(journeys, m)
    return m


FILE_TOOLS = {"Read", "Edit", "Write", "NotebookEdit", "Grep", "Glob"}
TEST_RUN_HINTS = ("cargo test", "npm test", "npm run test", "pytest", "vitest",
                  "playwright test", "go test", "just test", "cargo clippy")


def _enrich_file_signals(journeys: dict[str, list], m: Metrics) -> None:
    """Read-before-edit, test, and doc fractions from per-session tool journeys.

    The journey's `file` field is a path for file tools and the *command* for
    Bash — so a `cargo test` Bash step is a first-class testing signal, counted
    alongside touches of test files.
    """
    work_units = 0          # file touches + bash commands (the "stuff you did")
    test_signals = doc_signals = 0
    edits = edits_read_first = 0

    for seq in journeys.values():
        read_set: set[str] = set()
        for step in seq:
            tool = step.get("tool")
            val = step.get("file") or ""
            if not val:
                continue
            if tool == "Bash":
                work_units += 1
                if any(h in val.lower() for h in TEST_RUN_HINTS):
                    test_signals += 1
                continue
            if tool not in FILE_TOOLS:
                continue
            work_units += 1
            if _is_test_path(val):
                test_signals += 1
            if _is_doc_path(val):
                doc_signals += 1
            if tool == "Read":
                read_set.add(val)
            elif tool in ("Edit", "Write", "NotebookEdit"):
                edits += 1
                if val in read_set:
                    edits_read_first += 1

    if work_units:
        m.test_file_fraction = test_signals / work_units
        m.doc_file_fraction = doc_signals / work_units
    if edits:
        m.read_before_edit = edits_read_first / edits


# ── Rendering ───────────────────────────────────────────────────────────────
def bar(score: float, width: int = 20) -> str:
    filled = int(round(score / 10 * width))
    return "█" * filled + "·" * (width - filled)


def render_card(m: Metrics, scores: dict, archetype: str, rationale: str) -> str:
    edge, edge_v = growth_edge(scores)
    lines = []
    lines.append("╔══════════════════════════════════════════════════════╗")
    lines.append("║          YOUR OPEN STORY PROFILE  (local-only)       ║")
    lines.append("╚══════════════════════════════════════════════════════╝")
    lines.append("")
    lines.append(f"  ARCHETYPE   {archetype}")
    lines.append(f"              {rationale}")
    lines.append("")
    lines.append("  DIMENSIONS")
    for dim in DIMENSIONS:
        s = scores[dim]
        lines.append(f"    {dim:<12} {bar(s)}  {s:>4}")
    lines.append("")
    lines.append("  SIGNATURE")
    lines.append(f"    sessions          {m.session_count}  ·  {m.total_events} events")
    lines.append(f"    peak hour         {m.peak_hour:02d}:00   ({m.night_fraction*100:.0f}% nocturnal)")
    lines.append(f"    main project      {m.top_project}  (of {m.project_count})")
    lines.append(f"    throughput        {m.avg_tools_per_session:.0f} tool-calls / session")
    agents = ", ".join(f"{k} ×{v}" for k, v in sorted(m.agent_mix.items(), key=lambda kv: -kv[1]))
    lines.append(f"    agents            {agents}")
    lines.append("")
    lines.append(f"  GROWTH EDGE   {edge} ({edge_v}) — your lowest dimension.")
    lines.append("")
    return "\n".join(lines)


# ── Self-tests (the rubric is pure; assert on real values) ──────────────────
def run_tests() -> int:
    failures = []

    def check(name, cond):
        if not cond:
            failures.append(name)

    # norm clamps.
    check("norm-lo", norm(0, 10, 20) == 0.0)
    check("norm-hi", norm(30, 10, 20) == 10.0)
    check("norm-mid", norm(15, 10, 20) == 5.0)
    check("norm-inv", norm_inverse(40, tight=40, loose=600) == 10.0)

    # A "Velocity Machine": huge throughput, lots of writes, shallow planning.
    vel = Metrics(
        session_count=30, avg_tools_per_session=55, avg_events_per_session=120,
        write_intensity=0.45, read_intensity=0.1, delegation=0.12,
        plans_per_session=0.1, avg_scope_depth=0.2, planning_tools=0.0,
        read_before_edit=0.4, test_file_fraction=0.02, error_recovery=0.7,
        project_entropy=0.2, doc_file_fraction=0.02,
    )
    s = score_dimensions(vel)
    name, _ = classify(s)
    check("velocity-top", max(s, key=s.get) == "execution")
    check("velocity-name", name.startswith("Velocity"))

    # A "Quality Guardian": reads a lot, reads before editing, touches tests.
    qa = Metrics(
        session_count=20, avg_tools_per_session=22, avg_events_per_session=200,
        write_intensity=0.18, read_intensity=0.42, delegation=0.02,
        read_before_edit=0.9, test_file_fraction=0.28, error_recovery=0.95,
        plans_per_session=0.3, avg_scope_depth=0.5, planning_tools=0.01,
        project_entropy=0.3, doc_file_fraction=0.05,
    )
    s = score_dimensions(qa)
    check("quality-top", max(s, key=s.get) == "engineering")

    # An "Architect": plans everything, deep decomposition.
    arch = Metrics(
        session_count=15, avg_tools_per_session=14, avg_events_per_session=300,
        write_intensity=0.15, read_intensity=0.25, plans_per_session=1.8,
        avg_scope_depth=2.8, planning_tools=0.05, read_before_edit=0.8,
        test_file_fraction=0.1, error_recovery=0.8, project_entropy=0.2,
        doc_file_fraction=0.1,
    )
    s = score_dimensions(arch)
    check("architect-top", max(s, key=s.get) == "planning")

    # growth edge is the min.
    check("growth-edge", growth_edge({"a": 2.0, "b": 9.0})[0] == "a")

    # classify blends when within 0.5.
    name, _ = classify({"steering": 8.0, "execution": 7.7, "engineering": 1,
                        "product": 1, "planning": 1})
    check("hybrid", "hybrid" in name)

    if failures:
        print("FAIL:", ", ".join(failures))
        return 1
    print(f"ok — all profile-rubric tests passed")
    return 0


def build_profile(api: str = DEFAULT_API, days: int = 30, sample: int = 50) -> dict:
    """The one place that turns raw API data into a scored profile payload.

    Shared by the CLI (`main`) and the live HTML view (`profile_view.py`) so the
    rubric never forks. Returns a JSON-serializable dict; `{"empty": True}` when
    the window has no sessions.
    """
    m = collect_metrics(api, days, sample)
    if m.session_count == 0:
        return {"empty": True, "days": days, "api": api}
    scores = score_dimensions(m)
    archetype, rationale = classify(scores)
    return {
        "archetype": archetype,
        "rationale": rationale,
        "scores": scores,
        "dimensions": DIMENSIONS,
        "growth_edge": growth_edge(scores)[0],
        "days": days,
        "metrics": asdict(m),
    }


def main() -> int:
    ap = argparse.ArgumentParser(description="Local developer-profile prototype from OpenStory data")
    ap.add_argument("--days", type=int, default=30, help="window in days (default 30)")
    ap.add_argument("--sample", type=int, default=50, help="top-N sessions to deep-fetch (default 50)")
    ap.add_argument("--api", default=DEFAULT_API, help="OpenStory API base URL")
    ap.add_argument("--format", choices=["text", "json"], default="text")
    ap.add_argument("--test", action="store_true", help="run self-tests and exit")
    args = ap.parse_args()

    if args.test:
        return run_tests()

    payload = build_profile(args.api, args.days, args.sample)
    if payload.get("empty"):
        print(f"No sessions in the last {args.days} days. Is OpenStory running on {args.api}?")
        return 1

    if args.format == "json":
        print(json.dumps(payload, indent=2))
    else:
        m = Metrics(**payload["metrics"])
        print(render_card(m, payload["scores"], payload["archetype"], payload["rationale"]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
