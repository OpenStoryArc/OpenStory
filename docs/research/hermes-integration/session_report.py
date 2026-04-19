#!/usr/bin/env python3
"""
session_report.py — Produce a structural report on an OpenStory session.

Usage:
    python3 session_report.py <session_id>
    python3 session_report.py <session_id> --event <event_id>
    python3 session_report.py <session_id> --base-url http://remote:3002

Requires only the Python standard library. No pip install needed.

What this is
------------
This script is the procedural layer of the `session-report` skill (see
skills/session-report/SKILL.md in this directory). It composes the same
endpoints the recall tool sketch wraps (recall_tool_sketch.py) into a
single report-rendering pipeline aimed at humans rather than agents.

It exists because doing the same composition by hand — five curls, one
parsing bug, one failed-endpoint fallback, and a markdown render — is
the kind of pattern that should outlive the conversation that produced
it. Saving it here makes it reusable, and using it as a worked example
of "what calling the recall tool would actually look like" is the
secondary purpose.

Origin: extracted from a manual session report on 2026-04-08, captured in
docs/research/hermes-integration/SKILL_session-report.md. The procedural
sequence and the report structure are both load-bearing — the order
trades off cheap-and-broad against narrow-and-expensive (see the skill
file for the reasoning).
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.parse
import urllib.request
from typing import Any, Dict, List, Optional


DEFAULT_BASE_URL = os.environ.get("OPENSTORY_API_URL", "http://localhost:3002")
DEFAULT_TIMEOUT_S = 10.0


# ─── HTTP layer ──────────────────────────────────────────────────────────────


def _get(base: str, path: str, params: Optional[Dict[str, Any]] = None) -> Any:
    """One GET. Raises urllib errors on failure; callers handle."""
    url = base.rstrip("/") + path
    if params:
        kept = {k: v for k, v in params.items() if v is not None}
        if kept:
            url = url + "?" + urllib.parse.urlencode(kept)
    req = urllib.request.Request(url, headers={"Accept": "application/json"})
    with urllib.request.urlopen(req, timeout=DEFAULT_TIMEOUT_S) as resp:
        return json.loads(resp.read().decode("utf-8"))


def check_reachability(base: str) -> Optional[str]:
    """Return None if reachable, error message string if not."""
    try:
        _get(base, "/health")
        return None
    except urllib.error.URLError as e:
        return f"OpenStory unreachable at {base}: {e}. Is the server running?"
    except Exception as e:
        return f"Health check failed: {e}"


# ─── Section fetchers ────────────────────────────────────────────────────────


def fetch_session_profile(base: str, sid: str) -> Dict[str, Any]:
    """Combine /synopsis + /summary into one dict.

    These overlap by ~70% but each has unique fields:
      synopsis → label, top_tools, duration_secs, event_count, error_count
      summary  → status, model, prompt_count, response_count, exit_code

    Calling both is the explore-mode trade made by the original manual run.
    A future version that's confident about which fields it needs could
    drop one of the two calls.
    """
    profile: Dict[str, Any] = {}
    try:
        profile["synopsis"] = _get(base, f"/api/sessions/{sid}/synopsis")
    except Exception as e:
        profile["synopsis_error"] = str(e)
    try:
        profile["summary"] = _get(base, f"/api/sessions/{sid}/summary")
    except Exception as e:
        profile["summary_error"] = str(e)
    return profile


def fetch_file_impact(base: str, sid: str, top: int = 15) -> List[Dict[str, Any]]:
    files = _get(base, f"/api/sessions/{sid}/file-impact")
    if not isinstance(files, list):
        return []
    return files[:top]


def fetch_errors(base: str, sid: str) -> List[Any]:
    errs = _get(base, f"/api/sessions/{sid}/errors")
    return errs if isinstance(errs, list) else []


def fetch_turn_sentences(base: str, sid: str, limit: int = 50) -> List[Dict[str, Any]]:
    """Pull turn-sentence patterns.

    Note: the /patterns response is wrapped as {"patterns": [...]}, not a
    bare list. The original manual run learned this by trying to slice the
    response and getting a TypeError. Captured here so the script doesn't
    repeat the mistake — and so the next reader knows the convention.
    """
    resp = _get(
        base,
        f"/api/sessions/{sid}/patterns",
        {"type": "turn.sentence", "limit": limit},
    )
    if isinstance(resp, dict):
        return resp.get("patterns", [])
    if isinstance(resp, list):
        return resp
    return []


def find_event(base: str, sid: str, event_id: str) -> Optional[Dict[str, Any]]:
    """Locate one event by ID, with a fallback path.

    First tries /api/sessions/{sid}/events/{event_id}/content, the targeted
    endpoint. In the original manual run, this returned an empty body for
    the event we cared about — possibly because that endpoint expects a
    different ID format or only works for some event types. The script
    falls back to listing all events and grepping client-side.

    The fallback is expensive (~4MB for a 4000-event session) but reliable.
    A real recall tool implementation should fix the targeted endpoint or
    add a ?event_id= filter to /events instead of carrying this fallback
    forever.
    """
    # Targeted attempt — may return None or empty
    try:
        targeted = _get(base, f"/api/sessions/{sid}/events/{event_id}/content")
        if targeted:
            return targeted
    except Exception:
        pass

    # Fallback: pull listing and grep
    try:
        listing = _get(base, f"/api/sessions/{sid}/events", {"limit": 10000})
    except Exception as e:
        return {"_lookup_error": f"could not list events: {e}"}

    if isinstance(listing, dict):
        events = listing.get("events", listing.get("records", []))
    else:
        events = listing

    if not isinstance(events, list):
        return None

    for ev in events:
        if not isinstance(ev, dict):
            continue
        if ev.get("id") == event_id or ev.get("event_id") == event_id:
            return ev
    return None


# ─── Renderers — markdown sections ───────────────────────────────────────────


def _maybe_int(v: Any) -> Any:
    """Format ints with thousands separators; pass others through."""
    if isinstance(v, int):
        return f"{v:,}"
    return v


def render_at_a_glance(profile: Dict[str, Any]) -> str:
    syn = profile.get("synopsis") or {}
    summ = profile.get("summary") or {}

    lines = ["## At a glance", ""]
    label = syn.get("label", "(no label)")
    lines.append(f"**Label:** *{label}*")
    lines.append("")
    lines.append("| | |")
    lines.append("|---|---|")

    def row(k: str, v: Any) -> None:
        if v is not None and v != "":
            lines.append(f"| **{k}** | {v} |")

    row("Project", syn.get("project_name") or summ.get("project_id"))
    row("Status", summ.get("status"))
    row("Model", summ.get("model"))
    row("First event", syn.get("first_event") or summ.get("start_time"))
    row("Last event", syn.get("last_event"))
    duration_s = syn.get("duration_secs")
    if isinstance(duration_s, (int, float)) and duration_s > 0:
        hours = duration_s / 3600
        row("Duration", f"{hours:.1f} hours ({int(duration_s):,} seconds)")
    row("Events", _maybe_int(syn.get("event_count") or summ.get("event_count")))
    row("Tool calls", _maybe_int(syn.get("tool_count") or summ.get("tool_calls")))
    row("Errors", _maybe_int(syn.get("error_count") or summ.get("error_count")))
    row("Human prompts", _maybe_int(summ.get("prompt_count")))
    row("Assistant responses", _maybe_int(summ.get("response_count")))

    top_tools = syn.get("top_tools") or []
    if isinstance(top_tools, list) and top_tools:
        formatted = ", ".join(
            f"{t.get('tool', '?')} ({t.get('count', '?')})" for t in top_tools[:5]
        )
        row("Top tools", formatted)

    return "\n".join(lines)


def render_errors(errors: List[Any]) -> str:
    lines = ["## Errors", ""]
    if not errors:
        lines.append("**Zero errors.** Clean session.")
    else:
        lines.append(f"**{len(errors)} errors:**")
        lines.append("")
        for err in errors[:10]:
            if isinstance(err, dict):
                msg = err.get("message") or err.get("error") or json.dumps(err)
                lines.append(f"- {msg}")
            else:
                lines.append(f"- {err}")
        if len(errors) > 10:
            lines.append(f"- ... and {len(errors) - 10} more")
    return "\n".join(lines)


def _format_turn(p: Dict[str, Any]) -> str:
    md = p.get("metadata") or {}
    t = md.get("turn", "?")
    summary = p.get("summary", "(no summary)")
    return f"- **T{t}** — {summary}"


def render_arc(turns: List[Dict[str, Any]], head: int = 5, tail: int = 15) -> str:
    if not turns:
        return "## Turn arc\n\n*(no turn-sentence patterns surfaced)*"

    # Sort by metadata.turn for determinism. Patterns without a turn number
    # sort to the end.
    def turn_num(p: Dict[str, Any]) -> int:
        md = p.get("metadata") or {}
        n = md.get("turn")
        return n if isinstance(n, int) else 10 ** 9

    sorted_turns = sorted(turns, key=turn_num)

    lines = [f"## Turn arc — {len(sorted_turns)} turn-sentences surfaced", ""]

    if len(sorted_turns) <= head + tail:
        lines.append("### All turns")
        lines.append("")
        for p in sorted_turns:
            lines.append(_format_turn(p))
        return "\n".join(lines)

    lines.append(f"### First {head}")
    lines.append("")
    for p in sorted_turns[:head]:
        lines.append(_format_turn(p))
    lines.append("")
    lines.append(f"### Last {tail}")
    lines.append("")
    for p in sorted_turns[-tail:]:
        lines.append(_format_turn(p))
    return "\n".join(lines)


def render_files(files: List[Dict[str, Any]]) -> str:
    if not files:
        return "## Files most worked\n\n*(no file-impact data)*"

    lines = ["## Files most worked", ""]
    lines.append("| File | Reads | Writes |")
    lines.append("|---|---:|---:|")
    # Show the last 3 path components — fewer collides on common basenames
    # like src/state.rs (which appears under both rs/server/ and rs/store/),
    # more is unwieldy in a table.
    for f in files:
        path = f.get("file", "(unknown)")
        parts = path.rsplit("/", 3)
        short = "/".join(parts[-3:]) if len(parts) >= 3 else path
        lines.append(f"| `{short}` | {f.get('reads', 0)} | {f.get('writes', 0)} |")
    return "\n".join(lines)


def render_event(event_id: str, ev: Optional[Dict[str, Any]]) -> str:
    lines = [f"## Event `{event_id}`", ""]
    if ev is None:
        lines.append("*Event not found in session events list.*")
        return "\n".join(lines)
    if isinstance(ev, dict) and "_lookup_error" in ev:
        lines.append(f"*Lookup failed: {ev['_lookup_error']}*")
        return "\n".join(lines)

    # Top-level metadata
    if "time" in ev:
        lines.append(f"- **Time:** {ev['time']}")
    if "source" in ev:
        lines.append(f"- **Source:** {ev['source']}")
    if "type" in ev:
        lines.append(f"- **Type:** {ev['type']}")
    if "subtype" in ev:
        lines.append(f"- **Subtype:** {ev['subtype']}")

    data = ev.get("data") if isinstance(ev.get("data"), dict) else {}
    payload = data.get("agent_payload") if isinstance(data.get("agent_payload"), dict) else {}

    if payload:
        lines.append("")
        lines.append("**Agent payload (truncated to 2000 chars):**")
        lines.append("")
        lines.append("```json")
        lines.append(json.dumps(payload, indent=2)[:2000])
        lines.append("```")

    return "\n".join(lines)


# ─── Top-level orchestration ─────────────────────────────────────────────────


def build_report(base: str, session_id: str, event_id: Optional[str] = None) -> str:
    """Compose all sections into a single markdown report.

    Section order trades off cheap and broad → narrower and more specific:
      1. At-a-glance (synopsis + summary)
      2. Errors (cheap, surfaces a single important number)
      3. Turn arc (the narrative)
      4. Files (where the work landed)
      5. Specific event (only if --event was passed)

    Each section catches its own exceptions so a partial failure on one
    endpoint doesn't kill the rest of the report.
    """
    sections: List[str] = []

    try:
        profile = fetch_session_profile(base, session_id)
        sections.append(render_at_a_glance(profile))
    except Exception as e:
        sections.append(f"## At a glance\n\n*Failed to fetch profile: {e}*")

    try:
        errors = fetch_errors(base, session_id)
        sections.append(render_errors(errors))
    except Exception as e:
        sections.append(f"## Errors\n\n*Failed to fetch errors: {e}*")

    try:
        turns = fetch_turn_sentences(base, session_id)
        sections.append(render_arc(turns))
    except Exception as e:
        sections.append(f"## Turn arc\n\n*Failed to fetch turn sentences: {e}*")

    try:
        files = fetch_file_impact(base, session_id)
        sections.append(render_files(files))
    except Exception as e:
        sections.append(f"## Files most worked\n\n*Failed to fetch file impact: {e}*")

    if event_id:
        try:
            ev = find_event(base, session_id, event_id)
            sections.append(render_event(event_id, ev))
        except Exception as e:
            sections.append(f"## Event `{event_id}`\n\n*Lookup failed: {e}*")

    header = f"# Session report — `{session_id}`\n"
    return header + "\n\n" + "\n\n".join(sections) + "\n"


# ─── CLI ─────────────────────────────────────────────────────────────────────


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Produce a structural report on an OpenStory session.",
        epilog=(
            "Examples:\n"
            "  session_report.py f2679c73-79b1-4514-a9d3-c9a43e055822\n"
            "  session_report.py f2679c73-... --event c738865e-9668-418b-9e93-88a0a859d22e\n"
            "  session_report.py f2679c73-... --base-url http://remote:3002\n"
        ),
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("session_id", help="OpenStory session ID")
    parser.add_argument(
        "--event",
        default=None,
        metavar="EVENT_ID",
        help="Optional event ID to surface in detail",
    )
    parser.add_argument(
        "--base-url",
        default=DEFAULT_BASE_URL,
        help=f"OpenStory base URL (default: {DEFAULT_BASE_URL}, or $OPENSTORY_API_URL)",
    )
    args = parser.parse_args()

    err = check_reachability(args.base_url)
    if err:
        print(err, file=sys.stderr)
        return 1

    try:
        report = build_report(args.base_url, args.session_id, event_id=args.event)
    except KeyboardInterrupt:
        print("\nInterrupted.", file=sys.stderr)
        return 130
    except Exception as e:
        print(f"Report build failed: {type(e).__name__}: {e}", file=sys.stderr)
        return 2

    print(report)
    return 0


if __name__ == "__main__":
    sys.exit(main())
