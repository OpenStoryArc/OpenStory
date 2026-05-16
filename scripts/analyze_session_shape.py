#!/usr/bin/env python3
"""Classify the SICP-style 'process shape' of an agentic session.

The SICP framing: a procedure (the prompt) generates a process (the session).
Same procedure can generate radically different processes. This script asks:
  what shape did this session's process actually take?

Four shape components, each scored in [0, 1]:
  - tree         — subagent fan-out, parallel tool bursts (Fib-style branching)
  - iterative    — tight Read/Edit loops, stable working set (tail-recursive)
  - accumulating — TODOs/reads pile up faster than they close (linear-recursive)
  - narrowing    — working set contracts over time (logarithmic / bisect)

A session typically scores on multiple components. A 'dominant' label is only
emitted when one component clearly leads the others.

Usage:
  python3 scripts/analyze_session_shape.py --session <id>
  python3 scripts/analyze_session_shape.py --all
  python3 scripts/analyze_session_shape.py --test
"""

from __future__ import annotations

import argparse
import json
import math
import sys
import urllib.request
from collections import Counter
from dataclasses import asdict, dataclass, field
from typing import Any

API = "http://localhost:3002/api"


# ─────────────────────────────────────────────────────────────────
# Data
# ─────────────────────────────────────────────────────────────────


@dataclass
class ShapeFeatures:
    """Raw signals extracted from a session's records — the evidence layer."""

    session_id: str
    is_agent: bool
    total_records: int

    # Cycle structure
    user_prompts: int = 0
    assistant_messages: int = 0
    tool_calls: int = 0
    turn_ends: int = 0

    # Tool composition
    tool_counts: dict[str, int] = field(default_factory=dict)
    agent_spawns: int = 0
    bash_calls: int = 0
    read_calls: int = 0
    write_edit_calls: int = 0
    search_calls: int = 0

    # Burst structure (tools per assistant message)
    burst_widths: list[int] = field(default_factory=list)
    burst_width_avg: float = 0.0
    burst_width_max: int = 0
    bursts_with_3plus: int = 0

    # Bigram structure (consecutive tool pairs)
    bigram_entropy: float = 0.0
    top_bigram: str = ""
    top_bigram_share: float = 0.0
    read_edit_share: float = 0.0

    # Working set (distinct file paths touched, over time)
    distinct_files: int = 0
    working_set_growth: float = 0.0    # final / max in rolling window
    working_set_max: int = 0
    working_set_final: int = 0

    # Accumulation (opens vs closes)
    task_creates: int = 0
    task_completes: int = 0
    accumulation_ratio: float = 0.0    # creates / max(completes, 1)
    pending_tasks_at_end: int = 0      # creates - completes, floored at 0


@dataclass
class ShapeProfile:
    """Scored shape components + dominant label (or 'mixed' / 'empty')."""

    session_id: str
    features: ShapeFeatures

    tree_score: float = 0.0
    iterative_score: float = 0.0
    accumulating_score: float = 0.0
    narrowing_score: float = 0.0
    exploratory_score: float = 0.0   # fifth shape: stateless gathering / REPL-style

    dominant: str = "empty"
    notes: list[str] = field(default_factory=list)

    # Optional context — the session's first-prompt label, for cross-reference
    prompt_label: str = ""


# ─────────────────────────────────────────────────────────────────
# I/O
# ─────────────────────────────────────────────────────────────────


def fetch_records(sid: str) -> list[dict]:
    return json.loads(urllib.request.urlopen(f"{API}/sessions/{sid}/records").read())


def fetch_sessions() -> list[dict]:
    return json.loads(urllib.request.urlopen(f"{API}/sessions").read())["sessions"]


# ─────────────────────────────────────────────────────────────────
# Pure feature extraction
# ─────────────────────────────────────────────────────────────────


WRITE_TOOLS = {"Write", "Edit", "MultiEdit", "NotebookEdit"}
READ_TOOLS = {"Read"}
SEARCH_TOOLS = {"Grep", "Glob", "WebSearch", "WebFetch", "mcp__openstory__search"}
BASH_TOOLS = {"Bash"}
SUBAGENT_TOOLS = {"Agent", "Task"}

# Normalize agent-specific tool names to a shared vocabulary.
# Pi-mono uses snake_case; Claude Code uses CamelCase. Without this map,
# pi-mono sessions silently fall into weak-signal by measurement failure.
TOOL_NAME_NORM = {
    "exec": "Bash",
    "edit": "Edit",
    "read": "Read",
    "write": "Write",
    "web_search": "WebSearch",
    "web_fetch": "WebFetch",
}


def _normalize_tool_name(name: str) -> str:
    return TOOL_NAME_NORM.get(name, name)


def _tool_input(record: dict) -> dict:
    p = record.get("payload") or {}
    if isinstance(p, dict):
        inp = p.get("input") or {}
        if isinstance(inp, dict):
            return inp
    return {}


def _tool_name(record: dict) -> str:
    p = record.get("payload") or {}
    if isinstance(p, dict):
        return _normalize_tool_name(p.get("name", "?"))
    return "?"


def _file_path(tool_name: str, inp: dict) -> str | None:
    """Best-effort extraction of a file path from a tool input."""
    if tool_name in WRITE_TOOLS or tool_name in READ_TOOLS:
        v = inp.get("file_path")
        return v if isinstance(v, str) else None
    if tool_name in SEARCH_TOOLS:
        v = inp.get("path")
        return v if isinstance(v, str) else None
    return None


def _is_task_complete(inp: dict) -> bool:
    return inp.get("status") == "completed"


def extract_features(records: list[dict], session_id: str) -> ShapeFeatures:
    f = ShapeFeatures(
        session_id=session_id,
        is_agent=session_id.startswith("agent-"),
        total_records=len(records),
    )
    tool_counts: Counter[str] = Counter()
    tool_sequence: list[str] = []
    file_touch_seq: list[str] = []     # one entry per tool that touched a file
    burst = 0

    for r in records:
        rt = r.get("record_type", "?")
        if rt == "user_message":
            f.user_prompts += 1
        elif rt == "assistant_message":
            f.assistant_messages += 1
            if burst > 0:
                f.burst_widths.append(burst)
                burst = 0
        elif rt == "turn_end":
            f.turn_ends += 1
        elif rt == "tool_call":
            f.tool_calls += 1
            tname = _tool_name(r)
            tool_counts[tname] += 1
            tool_sequence.append(tname)
            burst += 1

            inp = _tool_input(r)
            if tname in SUBAGENT_TOOLS:
                f.agent_spawns += 1
            if tname in BASH_TOOLS:
                f.bash_calls += 1
            if tname in READ_TOOLS:
                f.read_calls += 1
            if tname in WRITE_TOOLS:
                f.write_edit_calls += 1
            if tname in SEARCH_TOOLS:
                f.search_calls += 1
            if tname == "TaskCreate" or tname == "TodoWrite":
                # TodoWrite is coarser; treat each call as one "create-ish" beat
                f.task_creates += 1
            if tname == "TaskUpdate" and _is_task_complete(inp):
                f.task_completes += 1

            fp = _file_path(tname, inp)
            if fp:
                file_touch_seq.append(fp)

    if burst > 0:
        f.burst_widths.append(burst)

    f.tool_counts = dict(tool_counts.most_common())

    if f.burst_widths:
        f.burst_width_avg = sum(f.burst_widths) / len(f.burst_widths)
        f.burst_width_max = max(f.burst_widths)
        f.bursts_with_3plus = sum(1 for w in f.burst_widths if w >= 3)

    # Bigrams over consecutive tool calls (not session-boundaried — close enough for v1)
    bigrams: Counter[tuple[str, str]] = Counter()
    for a, b in zip(tool_sequence, tool_sequence[1:]):
        bigrams[(a, b)] += 1
    total_bigrams = sum(bigrams.values())
    if total_bigrams > 0:
        # Shannon entropy in bits
        f.bigram_entropy = -sum(
            (n / total_bigrams) * math.log2(n / total_bigrams) for n in bigrams.values()
        )
        top_pair, top_n = bigrams.most_common(1)[0]
        f.top_bigram = f"{top_pair[0]}→{top_pair[1]}"
        f.top_bigram_share = top_n / total_bigrams
        read_edit = sum(
            n for (a, b), n in bigrams.items()
            if (a in READ_TOOLS and b in WRITE_TOOLS)
            or (a in WRITE_TOOLS and b in READ_TOOLS)
            or (a in READ_TOOLS and b in READ_TOOLS)
            or (a in WRITE_TOOLS and b in WRITE_TOOLS)
        )
        f.read_edit_share = read_edit / total_bigrams

    # Working-set trajectory: rolling window of last K distinct files
    seen_overall: set[str] = set()
    if file_touch_seq:
        K = 20
        rolling_max = 0
        for i, _ in enumerate(file_touch_seq):
            seen_overall.add(file_touch_seq[i])
            window = set(file_touch_seq[max(0, i - K + 1) : i + 1])
            rolling_max = max(rolling_max, len(window))
        final_window = set(file_touch_seq[-K:])
        f.distinct_files = len(seen_overall)
        f.working_set_max = rolling_max
        f.working_set_final = len(final_window)
        f.working_set_growth = (
            f.working_set_final / f.working_set_max if f.working_set_max else 0.0
        )

    # Accumulation
    f.accumulation_ratio = f.task_creates / max(f.task_completes, 1)
    f.pending_tasks_at_end = max(0, f.task_creates - f.task_completes)

    return f


# ─────────────────────────────────────────────────────────────────
# Pure classifier
# ─────────────────────────────────────────────────────────────────


def classify(f: ShapeFeatures) -> ShapeProfile:
    p = ShapeProfile(session_id=f.session_id, features=f)

    # Empty / trivial — bail out before scoring noise
    if f.tool_calls < 3:
        p.dominant = "empty" if f.tool_calls == 0 else "single-shot"
        p.notes.append(f"only {f.tool_calls} tool calls")
        return p

    # ── tree-recursive: subagent fan-out is the primary signal.
    # Absolute count, saturated at 5 spawns. Parallel bursts of 5+ tools count
    # as a weak secondary signal (vectorized iteration ≠ branching).
    agent_signal = min(1.0, f.agent_spawns / 5.0)
    very_wide_bursts = sum(1 for w in f.burst_widths if w >= 5)
    wide_signal = min(1.0, very_wide_bursts / 3.0)
    p.tree_score = min(1.0, agent_signal * 0.85 + wide_signal * 0.15)

    # ── iterative: Read/Edit-heavy bigrams + stable recent working set.
    # working_set_growth = final_window / max_window. High = the recent past
    # touches a small file set repeatedly = tail-recursive loop.
    p.iterative_score = max(
        0.0,
        min(1.0, f.read_edit_share * 1.5 + f.working_set_growth * 0.4 - 0.3),
    )

    # ── accumulating: opens outpace closes.
    # Primary signal: TaskCreate > TaskUpdate(completed). Fallback signal for
    # sessions that don't use Task tools: reads dwarf writes AND working set
    # keeps growing (final ≈ max, never contracting).
    accum_task_imbalance = min(1.0, max(0.0, (f.accumulation_ratio - 1.0) / 2.0))
    pending_signal = min(1.0, f.pending_tasks_at_end / 20)
    # Read/write imbalance as a fallback when no Task tool was used
    rw_imbalance = 0.0
    if f.task_creates == 0 and f.tool_calls >= 20:
        write_or_read = f.read_calls + f.write_edit_calls
        if write_or_read > 0:
            read_share = f.read_calls / write_or_read
            # read_share well above 0.7 + many files touched = gathering without committing
            if read_share > 0.7 and f.distinct_files >= 8:
                rw_imbalance = min(1.0, (read_share - 0.7) * 3)
    p.accumulating_score = min(
        1.0,
        accum_task_imbalance * 0.5 + pending_signal * 0.3 + rw_imbalance * 0.6,
    )

    # ── narrowing: working set contracts strongly (final / max small) AND search-led
    contraction = max(0.0, 1.0 - f.working_set_growth)
    search_lead = min(1.0, f.search_calls / max(f.tool_calls, 1) * 3)
    # Only score narrowing if there's actual file activity
    if f.distinct_files >= 3:
        p.narrowing_score = min(1.0, contraction * 0.6 + search_lead * 0.4) if f.working_set_growth < 0.4 else 0.0
    else:
        p.narrowing_score = 0.0

    # ── exploratory: high tool activity, almost no edits, no recursive scaffolding.
    # SICP doesn't name this shape because SICP processes compute values; this is
    # an instrumental loop — observation/gathering without accumulation.
    write_share = f.write_edit_calls / max(f.tool_calls, 1)
    gather_share = (f.bash_calls + f.search_calls + f.read_calls) / max(f.tool_calls, 1)
    stateless = (f.agent_spawns == 0 and f.task_creates == 0)
    if f.tool_calls >= 10:
        explore = max(0.0, gather_share - 0.4) * 1.5
        explore *= max(0.2, 1.0 - write_share * 2.5)
        explore *= 1.2 if stateless else 0.6
        p.exploratory_score = min(1.0, max(0.0, explore))
    else:
        p.exploratory_score = 0.0

    # Dominant: a component must lead by a clear margin
    scores = {
        "tree": p.tree_score,
        "iterative": p.iterative_score,
        "accumulating": p.accumulating_score,
        "narrowing": p.narrowing_score,
        "exploratory": p.exploratory_score,
    }
    ranked = sorted(scores.items(), key=lambda kv: -kv[1])
    top_name, top_score = ranked[0]
    second = ranked[1][1] if len(ranked) > 1 else 0.0
    if top_score < 0.25:
        p.dominant = "weak-signal"
    elif top_score - second >= 0.15:
        p.dominant = top_name
    else:
        # Two-shape mix — name both
        p.dominant = f"{ranked[0][0]}+{ranked[1][0]}"

    return p


# ─────────────────────────────────────────────────────────────────
# Trajectory — shape as a vector over time
# ─────────────────────────────────────────────────────────────────


@dataclass
class TrajectoryPoint:
    """Local shape in one window of the session timeline."""

    window_index: int
    fraction_through: float    # 0.0 = start, 1.0 = end
    record_range: tuple[int, int]
    profile: ShapeProfile


@dataclass
class Trajectory:
    """A session's directed path through shape-space."""

    session_id: str
    points: list[TrajectoryPoint]
    direction: str             # "iter→explore→tree→iter"
    overall: ShapeProfile      # the whole-session profile for comparison
    drift: dict[str, float]    # delta from first-half mean to second-half mean per axis


def compute_trajectory(records: list[dict], session_id: str, k: int = 10) -> Trajectory:
    """Window the session into k chunks; classify each chunk's local shape.

    Chunks are over *classifier-relevant* records (tool_call, asst/user message,
    turn_end). file_snapshot and system_event records can dominate raw counts
    without contributing to shape — windowing by raw index would leave most
    chunks visually empty when in fact tool activity is concentrated elsewhere.
    """
    relevant_kinds = {"tool_call", "assistant_message", "user_message", "turn_end"}
    filtered = [r for r in records if r.get("record_type") in relevant_kinds]
    n = len(filtered)

    points: list[TrajectoryPoint] = []
    if n == 0 or k <= 0:
        return Trajectory(
            session_id=session_id,
            points=points,
            direction="",
            overall=classify(extract_features(records, session_id)),
            drift={},
        )

    bounds = [(i * n) // k for i in range(k + 1)]
    for w in range(k):
        lo, hi = bounds[w], bounds[w + 1]
        if hi <= lo:
            continue
        slice_records = filtered[lo:hi]
        local_features = extract_features(slice_records, session_id)
        local_profile = classify(local_features)
        points.append(TrajectoryPoint(
            window_index=w,
            fraction_through=(lo + hi) / 2 / n,
            record_range=(lo, hi),
            profile=local_profile,
        ))

    # Direction string: collapse consecutive duplicate dominants
    raw = [pt.profile.dominant for pt in points]
    collapsed: list[str] = []
    for label in raw:
        if not collapsed or collapsed[-1] != label:
            collapsed.append(label)
    direction = "→".join(collapsed)

    # Drift: mean of each score in the second half minus the first half
    mid = len(points) // 2
    first_half = points[:mid] if mid > 0 else points
    second_half = points[mid:] if mid > 0 else points
    drift: dict[str, float] = {}
    for axis in ("tree", "iter", "accum", "narrow", "explore"):
        def _val(pt: TrajectoryPoint, ax: str) -> float:
            return {
                "tree": pt.profile.tree_score,
                "iter": pt.profile.iterative_score,
                "accum": pt.profile.accumulating_score,
                "narrow": pt.profile.narrowing_score,
                "explore": pt.profile.exploratory_score,
            }[ax]
        first_mean = sum(_val(p, axis) for p in first_half) / max(len(first_half), 1)
        second_mean = sum(_val(p, axis) for p in second_half) / max(len(second_half), 1)
        drift[axis] = second_mean - first_mean

    overall = classify(extract_features(records, session_id))
    return Trajectory(
        session_id=session_id,
        points=points,
        direction=direction,
        overall=overall,
        drift=drift,
    )


# ─────────────────────────────────────────────────────────────────
# Output
# ─────────────────────────────────────────────────────────────────


def print_profile(p: ShapeProfile, verbose: bool = False) -> None:
    f = p.features
    stype = "AGENT" if f.is_agent else "MAIN "
    print(f"\n{'='*78}")
    print(f"{stype} {p.session_id}  records={f.total_records}  tools={f.tool_calls}")
    print(f"{'='*78}")
    print(
        f"prompts={f.user_prompts}  asst={f.assistant_messages}  "
        f"turn_ends={f.turn_ends}  agents={f.agent_spawns}  "
        f"task_+/-={f.task_creates}/{f.task_completes}  files={f.distinct_files}"
    )
    print(f"top tools: {', '.join(f'{t}={n}' for t, n in list(f.tool_counts.items())[:5])}")
    print(
        f"bigram entropy={f.bigram_entropy:.2f}  top={f.top_bigram} ({f.top_bigram_share*100:.0f}%)  "
        f"R/W share={f.read_edit_share*100:.0f}%"
    )
    print(
        f"burst avg/max={f.burst_width_avg:.2f}/{f.burst_width_max}  "
        f"working-set max/final/growth={f.working_set_max}/{f.working_set_final}/{f.working_set_growth:.2f}"
    )
    print(
        f"\nSHAPE: tree={p.tree_score:.2f}  iter={p.iterative_score:.2f}  "
        f"accum={p.accumulating_score:.2f}  narrow={p.narrowing_score:.2f}  "
        f"explore={p.exploratory_score:.2f}  → DOMINANT: {p.dominant}"
    )
    for n in p.notes:
        print(f"  note: {n}")


def print_trajectory(traj: Trajectory) -> None:
    f = traj.overall.features
    print(f"\n{'='*100}")
    print(f"TRAJECTORY  {traj.session_id}  records={f.total_records}  tools={f.tool_calls}")
    print(f"{'='*100}")
    print(f"overall dominant: {traj.overall.dominant}")
    print(f"direction:        {traj.direction}")
    print(
        f"drift (first→second half):  "
        + "  ".join(f"{k}={v:+.2f}" for k, v in traj.drift.items())
    )
    print()
    print(f"{'w':>2} {'frac':>5}  {'dominant':<22}  tree  iter  accum narrow explore")
    print("-" * 80)
    for pt in traj.points:
        p = pt.profile
        print(
            f"{pt.window_index:>2} {pt.fraction_through:>5.2f}  {p.dominant:<22}  "
            f"{p.tree_score:>4.2f}  {p.iterative_score:>4.2f}  "
            f"{p.accumulating_score:>5.2f}  {p.narrowing_score:>5.2f}  {p.exploratory_score:>5.2f}"
        )


def print_table(profiles: list[ShapeProfile]) -> None:
    print(
        f"{'DOMINANT':<24} {'TREE':>5} {'ITER':>5} {'ACC':>5} {'NAR':>5} {'EXP':>5} {'TOOLS':>6} {'AG':>3}  PROMPT"
    )
    print("-" * 118)
    for p in profiles:
        f = p.features
        prompt = (p.prompt_label or p.session_id[:12])[:60]
        print(
            f"{p.dominant:<24} "
            f"{p.tree_score:>5.2f} {p.iterative_score:>5.2f} "
            f"{p.accumulating_score:>5.2f} {p.narrowing_score:>5.2f} "
            f"{p.exploratory_score:>5.2f} "
            f"{f.tool_calls:>6} {f.agent_spawns:>3}  {prompt}"
        )


# ─────────────────────────────────────────────────────────────────
# BDD-style invariant tests against live data
# ─────────────────────────────────────────────────────────────────


def _scenario(name: str, ok: bool, detail: str = "") -> tuple[bool, str]:
    return ok, f"{'PASS' if ok else 'FAIL'}  {name}{('  ('+detail+')') if detail else ''}"


def run_tests() -> int:
    sessions = fetch_sessions()
    # Sample up to 30 sessions of varied size — keep test runtime sane
    sessions.sort(key=lambda s: -s["event_count"])
    sample = sessions[:10] + sessions[len(sessions) // 2 : len(sessions) // 2 + 10] + sessions[-10:]
    seen: set[str] = set()
    profiles: list[ShapeProfile] = []
    for s in sample:
        sid = s["session_id"]
        if sid in seen:
            continue
        seen.add(sid)
        try:
            records = fetch_records(sid)
            profiles.append(classify(extract_features(records, sid)))
        except Exception as e:
            print(f"  SKIP {sid[:16]}: {e}")

    passed = 0
    failed = 0

    for p in profiles:
        f = p.features
        cases = []

        # Invariant 1: scores are in [0, 1]
        for name, v in (("tree", p.tree_score), ("iter", p.iterative_score),
                        ("accum", p.accumulating_score), ("narrow", p.narrowing_score)):
            cases.append(_scenario(
                f"[{p.session_id[:12]}] {name} score in [0,1]",
                0.0 <= v <= 1.0,
                f"got {v:.3f}",
            ))

        # Invariant 2: empty/trivial sessions get an empty/single-shot label
        if f.tool_calls < 3:
            cases.append(_scenario(
                f"[{p.session_id[:12]}] trivial → empty/single-shot",
                p.dominant in {"empty", "single-shot"},
                f"got {p.dominant}",
            ))

        # Invariant 3: heavy subagent spawners score >0 on tree
        if f.agent_spawns >= 3:
            cases.append(_scenario(
                f"[{p.session_id[:12]}] {f.agent_spawns} agent spawns → tree>0.0",
                p.tree_score > 0.0,
                f"tree={p.tree_score:.2f}",
            ))

        # Invariant 4: high read/write bigram share → iter contributes
        if f.read_edit_share > 0.5 and f.distinct_files <= 10:
            cases.append(_scenario(
                f"[{p.session_id[:12]}] R/W share {f.read_edit_share:.2f} + small files → iter>0",
                p.iterative_score > 0.0,
                f"iter={p.iterative_score:.2f}",
            ))

        # Invariant 5: accumulation_ratio > 1 implies accum > 0
        if f.accumulation_ratio > 1.5 and f.task_creates >= 5:
            cases.append(_scenario(
                f"[{p.session_id[:12]}] accum_ratio {f.accumulation_ratio:.1f} → accum>0",
                p.accumulating_score > 0.0,
                f"accum={p.accumulating_score:.2f}",
            ))

        # Invariant 6: total tool_calls equals sum of tool_counts values
        tc_sum = sum(f.tool_counts.values())
        cases.append(_scenario(
            f"[{p.session_id[:12]}] tool_counts sum = tool_calls",
            tc_sum == f.tool_calls,
            f"sum={tc_sum} calls={f.tool_calls}",
        ))

        # Invariant 7: dominant is one of the known labels
        valid_dominants = {"empty", "single-shot", "weak-signal", "tree", "iterative",
                           "accumulating", "narrowing", "exploratory"}
        is_pair = "+" in p.dominant and all(part in valid_dominants for part in p.dominant.split("+"))
        cases.append(_scenario(
            f"[{p.session_id[:12]}] dominant label is valid",
            p.dominant in valid_dominants or is_pair,
            f"got {p.dominant}",
        ))

        for ok, msg in cases:
            print("  " + msg)
            if ok:
                passed += 1
            else:
                failed += 1

    print(f"\n{passed} passed, {failed} failed across {len(profiles)} sessions")
    return 0 if failed == 0 else 1


# ─────────────────────────────────────────────────────────────────
# CLI
# ─────────────────────────────────────────────────────────────────


def cmd_distribution(top: int | None) -> None:
    """Aggregate dominant-label distribution across many sessions."""
    sessions = fetch_sessions()
    sessions.sort(key=lambda s: -s["event_count"])
    if top:
        sessions = sessions[:top]

    label_counts: Counter[str] = Counter()
    label_examples: dict[str, list[tuple[str, str, int]]] = {}
    subtype_breakdown: dict[str, Counter[str]] = {}
    mains_only: Counter[str] = Counter()
    agents_only: Counter[str] = Counter()
    feature_totals: dict[str, list[float]] = {}

    print(f"running classifier across {len(sessions)} sessions…", file=sys.stderr)
    n = 0
    for s in sessions:
        sid = s["session_id"]
        try:
            records = fetch_records(sid)
            prof = classify(extract_features(records, sid))
        except Exception as e:
            print(f"  SKIP {sid[:12]}: {e}", file=sys.stderr)
            continue
        n += 1
        dom = prof.dominant
        label_counts[dom] += 1
        label_examples.setdefault(dom, []).append(
            (sid, (s.get("label") or "")[:60], prof.features.tool_calls)
        )
        # Subtype: dominant-label split by primary tool family
        f = prof.features
        if f.tool_calls > 0:
            top_tool = next(iter(f.tool_counts), "?")
            family = _tool_family(top_tool)
            subtype_breakdown.setdefault(dom, Counter())[family] += 1
        if sid.startswith("agent-"):
            agents_only[dom] += 1
        else:
            mains_only[dom] += 1
        for k, v in (("tree", prof.tree_score), ("iter", prof.iterative_score),
                     ("accum", prof.accumulating_score), ("narrow", prof.narrowing_score),
                     ("explore", prof.exploratory_score)):
            feature_totals.setdefault(k, []).append(v)

    print(f"\n═══ shape distribution across {n} sessions ═══\n")
    print(f"{'DOMINANT':<25} {'TOTAL':>6} {'MAINS':>6} {'AGENTS':>7}  {'SUBTYPES (top-tool family)':<40}")
    print("-" * 90)
    for label, count in label_counts.most_common():
        subs = subtype_breakdown.get(label, Counter())
        sub_str = ", ".join(f"{fam}={n}" for fam, n in subs.most_common(4))
        print(f"{label:<25} {count:>6} {mains_only[label]:>6} {agents_only[label]:>7}  {sub_str:<40}")

    print("\n═══ score distributions (across all sessions) ═══")
    for k, vals in feature_totals.items():
        vals_sorted = sorted(vals)
        n_vals = len(vals_sorted)
        median = vals_sorted[n_vals // 2]
        p90 = vals_sorted[int(n_vals * 0.9)]
        nonzero = sum(1 for v in vals if v > 0.1)
        print(f"  {k:<8} median={median:.2f}  p90={p90:.2f}  >0.1: {nonzero}/{n_vals} sessions")

    print("\n═══ one example per dominant label ═══")
    for label, _ in label_counts.most_common():
        examples = label_examples.get(label, [])
        examples.sort(key=lambda e: -e[2])  # tallest tool count first
        if examples:
            sid, plabel, tools = examples[0]
            print(f"  {label:<25} {sid:<40} tools={tools:>4}  {plabel}")


def _tool_family(tool_name: str) -> str:
    """Group tool names into families for subtype analysis."""
    if tool_name in {"Read", "Write", "Edit", "MultiEdit"}:
        return "edit"
    if tool_name in {"Bash", "exec"}:
        return "bash"
    if tool_name in {"Agent", "Task"}:
        return "subagent"
    if tool_name in {"Grep", "Glob", "WebSearch", "WebFetch"}:
        return "search"
    if tool_name in {"TaskCreate", "TaskUpdate", "TodoWrite"}:
        return "task-mgmt"
    return "other"


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--session", help="Classify a single session by ID")
    p.add_argument("--trajectory", help="Emit windowed shape trajectory for SESSION (direction over time)")
    p.add_argument("--windows", type=int, default=10, help="Number of trajectory windows (default 10)")
    p.add_argument("--all", action="store_true", help="Classify all sessions, table output")
    p.add_argument("--distribution", action="store_true", help="Aggregate by dominant label across sessions")
    p.add_argument("--test", action="store_true", help="Run BDD-style invariant tests")
    p.add_argument("--verbose", "-v", action="store_true")
    p.add_argument("--top", type=int, default=20, help="With --all/--distribution, limit to top-N by event count")
    p.add_argument("--json", action="store_true", help="Emit profiles as JSON (with --session or --all)")
    args = p.parse_args()

    if args.test:
        sys.exit(run_tests())

    if args.distribution:
        cmd_distribution(args.top if args.top != 20 else None)
        return

    if args.trajectory:
        records = fetch_records(args.trajectory)
        traj = compute_trajectory(records, args.trajectory, k=args.windows)
        if args.json:
            print(json.dumps(asdict(traj), indent=2, default=str))
        else:
            print_trajectory(traj)
        return

    if args.session:
        records = fetch_records(args.session)
        prof = classify(extract_features(records, args.session))
        if args.json:
            print(json.dumps(asdict(prof), indent=2, default=str))
        else:
            print_profile(prof, verbose=args.verbose)
        return

    if args.all:
        sessions = fetch_sessions()
        sessions.sort(key=lambda s: -s["event_count"])
        profiles: list[ShapeProfile] = []
        for s in sessions[: args.top]:
            try:
                records = fetch_records(s["session_id"])
                prof = classify(extract_features(records, s["session_id"]))
                prof.prompt_label = (s.get("label") or "").strip()
                profiles.append(prof)
            except Exception as e:
                print(f"SKIP {s['session_id'][:16]}: {e}", file=sys.stderr)
        if args.json:
            print(json.dumps([asdict(pr) for pr in profiles], indent=2, default=str))
        else:
            print_table(profiles)
        return

    p.print_help()


if __name__ == "__main__":
    main()
