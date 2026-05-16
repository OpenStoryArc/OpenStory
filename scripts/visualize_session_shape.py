#!/usr/bin/env python3
"""Visualize an agentic session the way SICP draws a recursive process.

SICP's iconic images: the factorial trace (pyramid of deferred multiplications)
and the Fibonacci tree (branching sub-calls). We're after the same thing for
agentic sessions — make the process shape visually obvious.

Three modes:

  --profile SESSION
      Multi-track sparkline over the session timeline:
        TODOs    pending TaskCreate – TaskUpdate(completed). The deferred-op stack.
        agents   subagent spawns per bin. The branching events.
        workset  rolling distinct file count. Working-set trajectory.
        rate     tool calls per bin. Activity intensity.

  --trace SESSION
      Indented event log. Depth = pending TODOs at that moment. The SICP
      factorial trace, applied to a session.

  --compare A B
      Two --profile views side by side. The SICP "recursive vs iterative"
      diagram, applied to two sessions you want to contrast.

Uses only stdlib. Output is terminal-native ASCII/Unicode-block.
"""

from __future__ import annotations

import argparse
import json
import sys
import urllib.request
from collections import Counter
from dataclasses import dataclass, field

API = "http://localhost:3002/api"

# Sparkline ramp — eight levels for a continuous-looking trace
BLOCKS = " ▁▂▃▄▅▆▇█"


# ─────────────────────────────────────────────────────────────────
# Data
# ─────────────────────────────────────────────────────────────────


@dataclass
class Tracks:
    """Per-bin time series, one entry per output column."""

    bins: int
    pending_todos: list[int] = field(default_factory=list)  # stack height at end of bin
    agent_spawns: list[int] = field(default_factory=list)   # count per bin
    workset_size: list[int] = field(default_factory=list)   # distinct files in rolling window
    tool_rate: list[int] = field(default_factory=list)      # tool calls per bin


@dataclass
class TraceLine:
    depth: int     # pending TODOs at this point
    label: str     # human-readable event description
    kind: str      # 'prompt' | 'tool' | 'agent' | 'todo+' | 'todo-' | 'asst' | 'turn'


# ─────────────────────────────────────────────────────────────────
# I/O
# ─────────────────────────────────────────────────────────────────


def fetch_records(sid: str) -> list[dict]:
    return json.loads(urllib.request.urlopen(f"{API}/sessions/{sid}/records").read())


def fetch_session_meta(sid: str) -> dict:
    sessions = json.loads(urllib.request.urlopen(f"{API}/sessions").read())["sessions"]
    for s in sessions:
        if s["session_id"] == sid:
            return s
    return {"session_id": sid, "label": "", "event_count": 0}


# ─────────────────────────────────────────────────────────────────
# Pure trace extraction
# ─────────────────────────────────────────────────────────────────


SUBAGENT_TOOLS = {"Agent", "Task"}
FILE_TOOLS = {"Read", "Write", "Edit", "MultiEdit", "NotebookEdit"}
ROLLING_WINDOW = 20   # how many recent file touches define the working set


def _tool_name(r: dict) -> str:
    p = r.get("payload") or {}
    return p.get("name", "?") if isinstance(p, dict) else "?"


def _tool_input(r: dict) -> dict:
    p = r.get("payload") or {}
    if isinstance(p, dict):
        inp = p.get("input") or {}
        if isinstance(inp, dict):
            return inp
    return {}


def _is_completed(inp: dict) -> bool:
    return inp.get("status") == "completed"


def _file_path(tool_name: str, inp: dict) -> str | None:
    if tool_name in FILE_TOOLS:
        v = inp.get("file_path")
        return v if isinstance(v, str) else None
    return None


def _short(s: str, n: int = 60) -> str:
    s = (s or "").replace("\n", " ").strip()
    return s[: n - 1] + "…" if len(s) > n else s


def build_event_stream(records: list[dict]) -> list[dict]:
    """Project records to a slim per-event stream used by both modes."""
    events = []
    for r in records:
        rt = r.get("record_type", "?")
        if rt in {"user_message", "assistant_message", "tool_call", "turn_end"}:
            evt = {"rt": rt}
            if rt == "tool_call":
                evt["tool"] = _tool_name(r)
                evt["input"] = _tool_input(r)
            elif rt == "user_message":
                p = r.get("payload") or {}
                if isinstance(p, dict):
                    content = p.get("content")
                    if isinstance(content, str):
                        evt["text"] = content
                    elif isinstance(content, list):
                        for b in content:
                            if isinstance(b, dict) and b.get("type") == "text":
                                evt["text"] = b.get("text", "")
                                break
            events.append(evt)
    return events


def compute_tracks(events: list[dict], bins: int) -> Tracks:
    """Bucket the event stream into `bins` columns and accumulate per-track values."""
    tracks = Tracks(
        bins=bins,
        pending_todos=[0] * bins,
        agent_spawns=[0] * bins,
        workset_size=[0] * bins,
        tool_rate=[0] * bins,
    )
    if not events:
        return tracks

    n = len(events)
    pending = 0
    file_window: list[str] = []
    for i, e in enumerate(events):
        b = min(bins - 1, (i * bins) // n)
        rt = e["rt"]

        if rt == "tool_call":
            tracks.tool_rate[b] += 1
            tool = e.get("tool", "?")
            inp = e.get("input") or {}

            if tool in SUBAGENT_TOOLS:
                tracks.agent_spawns[b] += 1
            if tool == "TaskCreate" or tool == "TodoWrite":
                pending += 1
            if tool == "TaskUpdate" and _is_completed(inp):
                pending = max(0, pending - 1)

            fp = _file_path(tool, inp)
            if fp:
                file_window.append(fp)
                if len(file_window) > ROLLING_WINDOW:
                    file_window.pop(0)

        # pending and workset are *current state*, sample at every event into the bin
        tracks.pending_todos[b] = pending
        tracks.workset_size[b] = len(set(file_window))

    return tracks


def build_trace_lines(events: list[dict], max_lines: int) -> list[TraceLine]:
    """SICP-style indented trace. Depth = pending TODOs at the moment."""
    out: list[TraceLine] = []
    pending = 0

    for e in events:
        rt = e["rt"]

        if rt == "user_message":
            out.append(TraceLine(pending, f'prompt: "{_short(e.get("text", ""), 70)}"', "prompt"))
        elif rt == "assistant_message":
            # Implicit boundary between cycles — show as a faint marker only when there's been activity
            continue
        elif rt == "turn_end":
            out.append(TraceLine(pending, "── turn end ──", "turn"))
        elif rt == "tool_call":
            tool = e.get("tool", "?")
            inp = e.get("input") or {}

            if tool in SUBAGENT_TOOLS:
                desc = inp.get("description") or inp.get("subagent_type") or ""
                out.append(TraceLine(pending, f"⎇ spawn {tool} [{_short(str(desc), 50)}]", "agent"))
            elif tool == "TaskCreate" or tool == "TodoWrite":
                pending += 1
                subj = inp.get("subject") or inp.get("description") or "(todo)"
                out.append(TraceLine(pending, f"+ TODO: {_short(str(subj), 60)}", "todo+"))
            elif tool == "TaskUpdate" and _is_completed(inp):
                out.append(TraceLine(pending, f"✓ complete #{inp.get('taskId', '?')}", "todo-"))
                pending = max(0, pending - 1)
            else:
                summary = _tool_summary(tool, inp)
                out.append(TraceLine(pending, f"· {tool} {summary}", "tool"))

        if len(out) >= max_lines:
            out.append(TraceLine(pending, f"… (truncated at {max_lines} lines)", "tool"))
            break

    return out


def _tool_summary(tool: str, inp: dict) -> str:
    if tool in {"Read", "Write", "Edit"}:
        p = inp.get("file_path", "")
        return _short(p.split("/")[-1] if p else "", 50)
    if tool == "Bash":
        return _short(str(inp.get("command", "")), 60)
    if tool == "Grep":
        return _short(str(inp.get("pattern", "")), 50)
    if tool == "TaskUpdate":
        return f"#{inp.get('taskId', '?')} → {inp.get('status', '?')}"
    # Generic: first input key/value
    if inp:
        k = next(iter(inp))
        return _short(f"{k}={inp[k]}", 60)
    return ""


# ─────────────────────────────────────────────────────────────────
# Rendering
# ─────────────────────────────────────────────────────────────────


def sparkline(values: list[int], height_max: int | None = None) -> str:
    if not values:
        return ""
    hi = height_max if height_max is not None else max(values)
    if hi <= 0:
        return BLOCKS[0] * len(values)
    out = []
    for v in values:
        idx = 0 if v == 0 else min(len(BLOCKS) - 1, 1 + (v * (len(BLOCKS) - 2)) // hi)
        out.append(BLOCKS[idx])
    return "".join(out)


def render_profile(sid: str, label: str, tracks: Tracks) -> str:
    title = f"{sid}  —  {label}"
    sep = "─" * min(len(title), 100)
    lines = [sep, title, sep]

    def row(name: str, vals: list[int], unit: str = "") -> str:
        peak = max(vals) if vals else 0
        spark = sparkline(vals)
        return f"  {name:<8} {spark}  (peak={peak}{unit})"

    lines.append(row("TODOs", tracks.pending_todos, " pending"))
    lines.append(row("agents", tracks.agent_spawns, " spawns/bin"))
    lines.append(row("workset", tracks.workset_size, " files"))
    lines.append(row("rate", tracks.tool_rate, " tools/bin"))
    lines.append("")
    lines.append("  legend: TODOs=stack height (deferred ops)  agents=branching events")
    lines.append("          workset=rolling distinct files     rate=tool intensity")
    return "\n".join(lines)


def render_trace(sid: str, label: str, lines: list[TraceLine], max_depth_seen: int) -> str:
    title = f"{sid}  —  {label}"
    sep = "─" * min(len(title), 100)
    out = [sep, title, sep]
    out.append("  depth │ event")
    out.append("  ──────┼" + "─" * 80)
    for tl in lines:
        bar = "█" * tl.depth  # the SICP pyramid, made literal
        # color hint via symbol: agents are branches, todos are bricks, tools are dots
        out.append(f"  {tl.depth:>5} │ {bar}{' ' if bar else ''}{tl.label}")
    out.append("")
    out.append(f"  max depth reached: {max_depth_seen}")
    return "\n".join(out)


def render_compare(left: tuple[str, str, Tracks], right: tuple[str, str, Tracks]) -> str:
    """Print two profiles stacked (not literally side-by-side — terminal width matters)."""
    out = []
    for sid, label, tracks in (left, right):
        out.append(render_profile(sid, label, tracks))
        out.append("")
    return "\n".join(out)


# ─────────────────────────────────────────────────────────────────
# CLI
# ─────────────────────────────────────────────────────────────────


def cmd_profile(sid: str, bins: int) -> None:
    meta = fetch_session_meta(sid)
    records = fetch_records(sid)
    events = build_event_stream(records)
    tracks = compute_tracks(events, bins)
    print(render_profile(sid, _short(meta.get("label", ""), 80), tracks))


def cmd_trace(sid: str, max_lines: int) -> None:
    meta = fetch_session_meta(sid)
    records = fetch_records(sid)
    events = build_event_stream(records)
    lines = build_trace_lines(events, max_lines)
    max_depth = max((tl.depth for tl in lines), default=0)
    print(render_trace(sid, _short(meta.get("label", ""), 80), lines, max_depth))


def cmd_trajectory(sid: str, windows: int) -> None:
    """Per-axis sparkline of how the shape changes over the session."""
    # Defer-import to avoid coupling the visualizer to the classifier
    import subprocess
    proc = subprocess.run(
        [sys.executable, "scripts/analyze_session_shape.py",
         "--trajectory", sid, "--windows", str(windows), "--json"],
        capture_output=True, text=True, check=True,
    )
    traj = json.loads(proc.stdout)

    overall = traj.get("overall", {})
    print("─" * 100)
    print(f"TRAJECTORY  {sid}")
    print(f"overall dominant: {overall.get('dominant', '?')}")
    print(f"direction:        {traj.get('direction', '')}")
    drift = traj.get("drift", {})
    print(
        "drift (1st→2nd half):  "
        + "  ".join(f"{k}={v:+.2f}" for k, v in drift.items())
    )
    print("─" * 100)

    # Pull per-axis series from the trajectory points
    axes = [
        ("tree",    "tree_score"),
        ("iter",    "iterative_score"),
        ("accum",   "accumulating_score"),
        ("narrow",  "narrowing_score"),
        ("explore", "exploratory_score"),
    ]
    points = traj.get("points", [])
    for label, key in axes:
        vals = [int(round(pt["profile"][key] * 100)) for pt in points]
        spark = sparkline(vals, height_max=100)
        peak = max(vals) / 100 if vals else 0.0
        print(f"  {label:<8} {spark}  peak={peak:.2f}")

    # Per-window dominant labels under the sparklines
    dom_chars = []
    for pt in points:
        dom = pt["profile"]["dominant"]
        # First letter of primary component (t, i, a, n, e)
        dom_chars.append(dom[0] if dom else "·")
    print(f"  {'dom':<8} {''.join(dom_chars)}")
    print(f"  legend  t=tree i=iter a=accum n=narrow e=explore  w=weak-signal s=single-shot")


def cmd_compare(a: str, b: str, bins: int) -> None:
    meta_a = fetch_session_meta(a)
    meta_b = fetch_session_meta(b)
    tracks_a = compute_tracks(build_event_stream(fetch_records(a)), bins)
    tracks_b = compute_tracks(build_event_stream(fetch_records(b)), bins)
    print(render_compare(
        (a, _short(meta_a.get("label", ""), 80), tracks_a),
        (b, _short(meta_b.get("label", ""), 80), tracks_b),
    ))


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__.split("\n", 1)[0])
    p.add_argument("--profile", metavar="SESSION", help="Multi-track sparkline profile")
    p.add_argument("--trace", metavar="SESSION", help="Indented SICP-style event trace")
    p.add_argument("--trajectory", metavar="SESSION", help="Per-axis sparkline of shape over time")
    p.add_argument("--compare", nargs=2, metavar=("A", "B"), help="Two profiles stacked")
    p.add_argument("--bins", type=int, default=80, help="Sparkline width in columns (default 80)")
    p.add_argument("--windows", type=int, default=20, help="Trajectory window count (default 20)")
    p.add_argument("--max-lines", type=int, default=60, help="Trace mode line cap (default 60)")
    args = p.parse_args()

    if args.profile:
        cmd_profile(args.profile, args.bins)
    elif args.trace:
        cmd_trace(args.trace, args.max_lines)
    elif args.trajectory:
        cmd_trajectory(args.trajectory, args.windows)
    elif args.compare:
        cmd_compare(args.compare[0], args.compare[1], args.bins)
    else:
        p.print_help()
        sys.exit(1)


if __name__ == "__main__":
    main()
